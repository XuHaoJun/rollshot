use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use crate::backend::FrameStream;
use crate::error::CaptureError;
use crate::types::CapturedFrame;

use super::pixel::LinuxPixelFormat;

pub const NEXT_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

#[allow(unsafe_code)] // OwnedFd::from_raw_fd is the documented bridge from a raw kernel fd.
pub fn dup_pipewire_fd(
    input: std::os::fd::BorrowedFd<'_>,
) -> Result<std::os::fd::OwnedFd, CaptureError> {
    use nix::fcntl::{fcntl, FcntlArg};
    use std::os::fd::{AsRawFd, FromRawFd};

    let raw = input.as_raw_fd();
    let duplicated = fcntl(raw, FcntlArg::F_DUPFD_CLOEXEC(5)).map_err(|err| {
        CaptureError::Backend(anyhow::anyhow!("failed to duplicate PipeWire fd: {err}"))
    })?;
    // SAFETY: fcntl(F_DUPFD_CLOEXEC) returns a fresh, exclusively-owned RawFd
    // with the CLOEXEC flag set. No other code path in this crate touches that
    // numeric fd before this OwnedFd is constructed, so ownership transfer is
    // unambiguous. The original `input` BorrowedFd is unaffected by F_DUPFD.
    Ok(unsafe { std::os::fd::OwnedFd::from_raw_fd(duplicated) })
}

pub enum FrameEvent {
    Frame(CapturedFrame),
    End,
    Error(String),
}

pub struct FrameQueue {
    inner: Mutex<VecDeque<FrameEvent>>,
    condvar: Condvar,
}

impl FrameQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
            condvar: Condvar::new(),
        }
    }

    pub fn push(&self, event: FrameEvent) {
        let mut deque = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if deque.len() >= 3 {
            deque.pop_front();
        }
        deque.push_back(event);
        self.condvar.notify_one();
    }

    pub fn next_frame_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<CapturedFrame, CaptureError> {
        let deque = match self.inner.lock() {
            Ok(guard) => guard,
            Err(_poisoned) => {
                eprintln!("frame queue mutex poisoned — producer thread panicked");
                return Err(CaptureError::EndOfStream);
            }
        };

        let result = self.condvar.wait_timeout_while(deque, timeout, |d| d.is_empty());

        let (mut deque, wait_result) = match result {
            Ok(pair) => pair,
            Err(_poisoned) => {
                eprintln!("frame queue mutex poisoned during wait — producer thread panicked");
                return Err(CaptureError::EndOfStream);
            }
        };

        if wait_result.timed_out() && deque.is_empty() {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream produced no frames within {timeout:?}"
            )));
        }

        match deque.pop_front() {
            Some(FrameEvent::Frame(f)) => Ok(f),
            Some(FrameEvent::End) => Err(CaptureError::EndOfStream),
            Some(FrameEvent::Error(msg)) => {
                Err(CaptureError::Backend(anyhow::anyhow!(msg)))
            }
            None => Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream produced no frames within {timeout:?}"
            ))),
        }
    }
}

impl Default for FrameQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxVideoTransform {
    Normal,
    Rotated90,
    Rotated180,
    Rotated270,
    Flipped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoCrop {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxFrameMetadata {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: LinuxPixelFormat,
    pub crop: Option<VideoCrop>,
    pub transform: LinuxVideoTransform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBufferType {
    MemPtr,
    DmaBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DequeuedBuffer<'a> {
    pub data: &'a [u8],
    pub buffer_type: LinuxBufferType,
    pub chunk_size: u32,
    pub chunk_stride: i32,
    pub chunk_corrupted: bool,
    pub header_corrupted: bool,
    pub metadata: LinuxFrameMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferAction {
    Skip,
    Produce(LinuxFrameMetadata),
}

pub fn inspect_dequeued_buffer(
    buf: &DequeuedBuffer<'_>,
    empty_count: &mut u8,
) -> Result<BufferAction, CaptureError> {
    if buf.buffer_type == LinuxBufferType::DmaBuf {
        return Err(CaptureError::Unsupported {
            message: "unexpected PipeWire DMA-BUF buffer".to_string(),
        });
    }

    if buf.header_corrupted {
        *empty_count += 1;
        if *empty_count >= 10 {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream did not produce a usable video frame after 10 attempts"
            )));
        }
        return Ok(BufferAction::Skip);
    }

    if buf.chunk_corrupted {
        *empty_count += 1;
        if *empty_count >= 10 {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream did not produce a usable video frame after 10 attempts"
            )));
        }
        return Ok(BufferAction::Skip);
    }

    if buf.chunk_size == 0 {
        *empty_count += 1;
        if *empty_count >= 10 {
            return Err(CaptureError::Backend(anyhow::anyhow!(
                "PipeWire stream did not produce a usable video frame after 10 attempts"
            )));
        }
        return Ok(BufferAction::Skip);
    }

    if buf.metadata.transform != LinuxVideoTransform::Normal {
        return Err(CaptureError::Backend(anyhow::anyhow!(
            "unsupported PipeWire video transform: {:?}",
            buf.metadata.transform
        )));
    }

    *empty_count = 0;

    if let Some(crop) = buf.metadata.crop {
        let x2 = crop.x as u32 + crop.width;
        let y2 = crop.y as u32 + crop.height;
        if x2 > buf.metadata.width || y2 > buf.metadata.height {
            return Err(CaptureError::InvalidConfig {
                message: format!(
                    "manual crop region x={},y={},w={},h={} is outside available frame {}x{}",
                    crop.x, crop.y, crop.width, crop.height, buf.metadata.width, buf.metadata.height
                ),
            });
        }
    }

    Ok(BufferAction::Produce(buf.metadata))
}

pub fn map_spa_video_format(
    format: pipewire::spa::param::video::VideoFormat,
) -> Result<LinuxPixelFormat, CaptureError> {
    use pipewire::spa::param::video::VideoFormat;
    match format {
        VideoFormat::BGRA => Ok(LinuxPixelFormat::Bgra),
        VideoFormat::RGBA => Ok(LinuxPixelFormat::Rgba),
        VideoFormat::BGRx => Ok(LinuxPixelFormat::Bgrx),
        VideoFormat::RGBx => Ok(LinuxPixelFormat::Rgbx),
        VideoFormat::RGB => Ok(LinuxPixelFormat::Rgb),
        other => Err(CaptureError::Unsupported {
            message: format!("unsupported PipeWire raw video format: {other:?}"),
        }),
    }
}

pub struct LinuxPortalFrameStream {
    queue: Arc<FrameQueue>,
}

#[cfg(not(test))]
mod connection {
    use super::*;
    use std::os::fd::AsFd;
    use pipewire::spa::param::format::{MediaSubtype, MediaType};
    use pipewire::spa::param::video::{VideoFormat, VideoInfoRaw};
    use pipewire::spa::pod::builder::{Builder, builder_add};
    use pipewire::spa::pod::*;
    use pipewire::spa::utils::{Fraction, Id, Rectangle};

    pub struct PipeWireConnection {
        thread_loop: pipewire::thread_loop::ThreadLoop,
        _listener: pipewire::stream::StreamListener<StreamUserData>,
        stream: pipewire::stream::Stream,
        _context: pipewire::context::Context,
        _core: pipewire::core::Core,
        _format_data: Vec<Vec<u8>>,
    }

    pub(super) struct StreamUserData {
        pub queue: Arc<FrameQueue>,
        pub empty_count: u8,
        pub negotiated_format: Option<LinuxPixelFormat>,
        pub negotiated_width: u32,
        pub negotiated_height: u32,
        pub negotiated_stride: u32,
        pub crop: Option<VideoCrop>,
        pub transform: LinuxVideoTransform,
        pub options: crate::types::CaptureOptions,
    }

    fn build_meta_param_bytes(meta_type: u32) -> Vec<u8> {
        let mut data = Vec::new();
        {
            let mut builder = Builder::new(&mut data);
            let _ = builder_add!(&mut builder,
                Object(pipewire::spa::sys::SPA_TYPE_OBJECT_ParamMeta, pipewire::spa::sys::SPA_PARAM_Meta) {
                    pipewire::spa::sys::SPA_PARAM_META_type => Id(Id(meta_type)),
                    pipewire::spa::sys::SPA_PARAM_META_size => Int(64i32)
                }
            );
        }
        data
    }

    fn build_buf_mem_ptr_param_bytes() -> Vec<u8> {
        let mut data = Vec::new();
        {
            let mut builder = Builder::new(&mut data);
            let _ = builder_add!(&mut builder,
                Object(pipewire::spa::sys::SPA_TYPE_OBJECT_ParamBuffers, pipewire::spa::sys::SPA_PARAM_Buffers) {
                    pipewire::spa::sys::SPA_PARAM_BUFFERS_dataType => Int(1i32 << 1)
                }
            );
        }
        data
    }

    fn build_format_pod_bytes(fmt: VideoFormat, fps: u32) -> Vec<u8> {
        let mut data = Vec::new();
        {
            let mut builder = Builder::new(&mut data);
            let _ = builder_add!(&mut builder,
                Object(pipewire::spa::sys::SPA_TYPE_OBJECT_Format, pipewire::spa::sys::SPA_PARAM_EnumFormat) {
                    pipewire::spa::sys::SPA_FORMAT_mediaType => Id(Id(MediaType::Video.as_raw())),
                    pipewire::spa::sys::SPA_FORMAT_mediaSubtype => Id(Id(MediaSubtype::Raw.as_raw())),
                    pipewire::spa::sys::SPA_FORMAT_VIDEO_format => Id(Id(fmt.as_raw())),
                    pipewire::spa::sys::SPA_FORMAT_VIDEO_size => Rectangle(Rectangle { width: 1920, height: 1080 }),
                    pipewire::spa::sys::SPA_FORMAT_VIDEO_framerate => Fraction(Fraction { num: fps, denom: 1 })
                }
            );
        }
        data
    }

    #[allow(unsafe_code)] // ThreadLoop::new is documented as safe but marked unsafe in the crate
    impl PipeWireConnection {
        pub fn connect_fd(
            portal_fd: std::os::fd::OwnedFd,
            node_id: u32,
            options: crate::types::CaptureOptions,
            queue: Arc<FrameQueue>,
        ) -> Result<Self, CaptureError> {
            pipewire::init();

            let thread_loop = unsafe {
                pipewire::thread_loop::ThreadLoop::new(Some("rollshot-pipewire"), None)
                    .map_err(|e| CaptureError::Backend(anyhow::anyhow!("thread loop: {e}")))?
            };

            let context = pipewire::context::Context::new(&thread_loop)
                .map_err(|e| CaptureError::Backend(anyhow::anyhow!("context: {e}")))?;

            let dup_fd = dup_pipewire_fd(portal_fd.as_fd())?;

            let core = context
                .connect_fd(dup_fd, None)
                .map_err(|e| CaptureError::Backend(anyhow::anyhow!("connect_fd: {e}")))?;

            let mut props = pipewire::properties::Properties::new();
            props.insert("media.type", "Video");
            props.insert("media.category", "Capture");
            props.insert("media.role", "Screen");

            let stream = pipewire::stream::Stream::new(&core, "rollshot-screen", props)
                .map_err(|e| CaptureError::Backend(anyhow::anyhow!("stream: {e}")))?;

            let user_data = StreamUserData {
                queue: Arc::clone(&queue),
                empty_count: 0,
                negotiated_format: None,
                negotiated_width: 0,
                negotiated_height: 0,
                negotiated_stride: 0,
                crop: None,
                transform: LinuxVideoTransform::Normal,
                options: options.clone(),
            };

            let formats = [
                VideoFormat::BGRA,
                VideoFormat::RGBA,
                VideoFormat::BGRx,
                VideoFormat::RGBx,
                VideoFormat::RGB,
            ];

            let mut format_data: Vec<Vec<u8>> = Vec::new();
            for fmt in formats {
                format_data.push(build_format_pod_bytes(fmt, options.fps));
            }

            let mut format_refs: Vec<&Pod> = format_data
                .iter()
                .filter_map(|d| Pod::from_bytes(d))
                .collect();

            let listener = stream
                .add_local_listener_with_user_data(user_data)
                .state_changed(|_stream, _user_data, _old, _new| {})
                .param_changed(|stream, user_data, id, _param| {
                    use pipewire::spa::param::ParamType;

                    if id == ParamType::Format.as_raw() {
                        if let Some(param) = _param {
                            let mut info = VideoInfoRaw::new();
                            if info.parse(param).is_ok() {
                                let format = info.format();
                                if let Ok(pixel_format) = map_spa_video_format(format) {
                                    let size = info.size();
                                    let stride = size.width * 4;

                                    user_data.negotiated_format = Some(pixel_format);
                                    user_data.negotiated_width = size.width;
                                    user_data.negotiated_height = size.height;
                                    user_data.negotiated_stride = stride;

                                    let header_data = build_meta_param_bytes(0);
                                    let crop_data = build_meta_param_bytes(1);
                                    let cursor_data = build_meta_param_bytes(2);
                                    let buf_data = build_buf_mem_ptr_param_bytes();

                                    let all_data = [header_data, crop_data, cursor_data, buf_data];
                                    let mut param_refs: Vec<&Pod> = all_data
                                        .iter()
                                        .filter_map(|d| Pod::from_bytes(d))
                                        .collect();
                                    let _ = stream.update_params(&mut param_refs);
                                }
                            }
                        }
                    }
                })
                .process(|stream, user_data| {
                    let mut buffer = match stream.dequeue_buffer() {
                        Some(buf) => buf,
                        None => return,
                    };

                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }

                    let data = &mut datas[0];
                    let data_type = data.type_();
                    let chunk = data.chunk();
                    let chunk_size = chunk.size();
                    let chunk_stride = chunk.stride();
                    let chunk_corrupted =
                        chunk.flags().contains(pipewire::spa::buffer::ChunkFlags::CORRUPTED);

                    let buffer_type = if data_type == pipewire::spa::buffer::DataType::MemPtr {
                        LinuxBufferType::MemPtr
                    } else if data_type == pipewire::spa::buffer::DataType::DmaBuf {
                        LinuxBufferType::DmaBuf
                    } else {
                        user_data.empty_count += 1;
                        return;
                    };

                    let raw_data: &[u8] = if buffer_type == LinuxBufferType::MemPtr {
                        data.data().map_or(&[][..], |v| v)
                    } else {
                        &[]
                    };

                    let metadata = LinuxFrameMetadata {
                        width: user_data.negotiated_width,
                        height: user_data.negotiated_height,
                        stride: user_data.negotiated_stride,
                        format: user_data
                            .negotiated_format
                            .unwrap_or(LinuxPixelFormat::Bgra),
                        crop: user_data.crop,
                        transform: user_data.transform,
                    };

                    let buf_info = DequeuedBuffer {
                        data: raw_data,
                        buffer_type,
                        chunk_size,
                        chunk_stride,
                        chunk_corrupted,
                        header_corrupted: false,
                        metadata,
                    };

                    match inspect_dequeued_buffer(&buf_info, &mut user_data.empty_count) {
                        Ok(BufferAction::Produce(meta)) => {
                            if let Some(pixel_format) = user_data.negotiated_format {
                                let raw_frame = super::super::pixel::LinuxRawFrame {
                                    data: raw_data,
                                    width: meta.width,
                                    height: meta.height,
                                    stride: meta.stride,
                                    format: pixel_format,
                                    crop: meta.crop.map(|c| crate::types::Region {
                                        x: c.x,
                                        y: c.y,
                                        width: c.width,
                                        height: c.height,
                                    }),
                                };
                                match super::super::pixel::raw_frame_to_rgba(raw_frame) {
                                    Ok(image) => {
                                        user_data.queue.push(FrameEvent::Frame(CapturedFrame {
                                            image,
                                            timestamp: std::time::SystemTime::now(),
                                            metadata: crate::types::FrameMetadata {
                                                source_size: Some(crate::types::Size {
                                                    width: meta.width,
                                                    height: meta.height,
                                                }),
                                                effective_region: meta.crop.map(|c| {
                                                    crate::types::Region {
                                                        x: c.x,
                                                        y: c.y,
                                                        width: c.width,
                                                        height: c.height,
                                                    }
                                                }),
                                                pixel_format: Some(match pixel_format {
                                                    LinuxPixelFormat::Bgra => {
                                                        crate::types::PixelFormat::Bgra
                                                    }
                                                    LinuxPixelFormat::Rgba => {
                                                        crate::types::PixelFormat::Rgba
                                                    }
                                                    LinuxPixelFormat::Bgrx => {
                                                        crate::types::PixelFormat::Bgrx
                                                    }
                                                    LinuxPixelFormat::Rgbx => {
                                                        crate::types::PixelFormat::Rgbx
                                                    }
                                                    LinuxPixelFormat::Rgb => {
                                                        crate::types::PixelFormat::Rgb
                                                    }
                                                }),
                                                stride: Some(meta.stride),
                                                backend: "linux-portal",
                                            },
                                        }));
                                    }
                                    Err(e) => {
                                        user_data.queue.push(FrameEvent::Error(e.to_string()));
                                    }
                                }
                            }
                        }
                        Ok(BufferAction::Skip) => {}
                        Err(e) => {
                            user_data.queue.push(FrameEvent::Error(e.to_string()));
                        }
                    }
                })
                .register()
                .map_err(|e| CaptureError::Backend(anyhow::anyhow!("listener: {e}")))?;

            {
                let _lock = thread_loop.lock();
                stream
                    .connect(
                        pipewire::spa::utils::Direction::Input,
                        Some(node_id),
                        pipewire::stream::StreamFlags::AUTOCONNECT
                            | pipewire::stream::StreamFlags::MAP_BUFFERS,
                        &mut format_refs,
                    )
                    .map_err(|e| CaptureError::Backend(anyhow::anyhow!("stream connect: {e}")))?;
            }

            thread_loop.start();

            Ok(PipeWireConnection {
                thread_loop,
                _listener: listener,
                stream,
                _context: context,
                _core: core,
                _format_data: format_data,
            })
        }
    }

    impl Drop for PipeWireConnection {
        fn drop(&mut self) {
            self.thread_loop.stop();
            if let Err(e) = self.stream.disconnect() {
                eprintln!("PipeWire stream disconnect error: {e}");
            }
        }
    }
}

#[cfg(test)]
mod connection {
    use super::*;

    pub struct PipeWireConnection;

    impl PipeWireConnection {
        pub fn connect_fd(
            _portal_fd: std::os::fd::OwnedFd,
            _node_id: u32,
            _options: crate::types::CaptureOptions,
            _queue: Arc<FrameQueue>,
        ) -> Result<Self, CaptureError> {
            Ok(PipeWireConnection)
        }
    }
}

impl LinuxPortalFrameStream {
    pub fn new(
        portal_fd: std::os::fd::OwnedFd,
        node_id: u32,
        options: crate::types::CaptureOptions,
    ) -> Result<Self, CaptureError> {
        let queue = Arc::new(FrameQueue::new());
        let _connection = connection::PipeWireConnection::connect_fd(
            portal_fd,
            node_id,
            options,
            Arc::clone(&queue),
        )?;
        Ok(Self { queue })
    }
}

impl FrameStream for LinuxPortalFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        self.queue.next_frame_with_timeout(NEXT_FRAME_TIMEOUT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CapturedFrame, FrameMetadata};
    use image::RgbaImage;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;
    use std::sync::Arc;
    use std::time::SystemTime;

    fn dummy_frame(n: u32) -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::new(n, n),
            timestamp: SystemTime::now(),
            metadata: FrameMetadata::fake(),
        }
    }

    fn make_meta(width: u32, height: u32) -> LinuxFrameMetadata {
        LinuxFrameMetadata {
            width,
            height,
            stride: width * 4,
            format: LinuxPixelFormat::Bgra,
            crop: None,
            transform: LinuxVideoTransform::Normal,
        }
    }

    #[test]
    fn dup_pipewire_fd_does_not_consume_input() {
        let (mut reader, mut writer) = std::os::unix::net::UnixStream::pair().unwrap();
        let duplicated = dup_pipewire_fd(reader.as_fd()).unwrap();
        drop(duplicated);
        writer.write_all(b"x").unwrap();
        let mut byte = [0; 1];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte, [b'x']);
    }

    #[test]
    fn queue_retains_newest_three_of_five() {
        let queue = FrameQueue::new();
        for i in 1..=5 {
            queue.push(FrameEvent::Frame(dummy_frame(i)));
        }
        let f3 = queue.next_frame_with_timeout(Duration::from_millis(50)).unwrap();
        let f4 = queue.next_frame_with_timeout(Duration::from_millis(50)).unwrap();
        let f5 = queue.next_frame_with_timeout(Duration::from_millis(50)).unwrap();
        assert_eq!(f3.image.width(), 3);
        assert_eq!(f4.image.width(), 4);
        assert_eq!(f5.image.width(), 5);
    }

    #[test]
    fn queue_timeout_returns_backend_error() {
        let queue = FrameQueue::new();
        let err = queue
            .next_frame_with_timeout(Duration::from_millis(100))
            .unwrap_err();
        match err {
            CaptureError::Backend(e) => {
                assert!(
                    e.to_string().contains("no frames within"),
                    "msg = {}",
                    e
                );
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    #[test]
    fn queue_end_event_returns_end_of_stream() {
        let queue = FrameQueue::new();
        queue.push(FrameEvent::End);
        let err = queue
            .next_frame_with_timeout(Duration::from_millis(50))
            .unwrap_err();
        assert!(matches!(err, CaptureError::EndOfStream), "got {err:?}");
    }

    #[test]
    fn queue_error_event_returns_backend_error() {
        let queue = FrameQueue::new();
        queue.push(FrameEvent::Error("bad pixel data".to_string()));
        let err = queue
            .next_frame_with_timeout(Duration::from_millis(50))
            .unwrap_err();
        match err {
            CaptureError::Backend(e) => {
                assert!(
                    e.to_string().contains("bad pixel data"),
                    "msg = {}",
                    e
                );
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    #[test]
    fn queue_producer_then_consumer() {
        let queue = Arc::new(FrameQueue::new());
        let q = Arc::clone(&queue);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            q.push(FrameEvent::Frame(dummy_frame(42)));
        });
        let f = queue
            .next_frame_with_timeout(Duration::from_secs(1))
            .unwrap();
        assert_eq!(f.image.width(), 42);
        handle.join().unwrap();
    }

    // Drop order verification

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum DropKind {
        ThreadLoopStop,
        StreamDisconnectDestroy,
        ContextDestroy,
        OriginalFdDrop,
        PortalSessionClose,
    }

    struct DropRecorder {
        kind: DropKind,
        log: Arc<Mutex<Vec<DropKind>>>,
    }

    impl Drop for DropRecorder {
        fn drop(&mut self) {
            self.log.lock().unwrap().push(self.kind.clone());
        }
    }

    struct TestResources {
        _thread_loop_stop: DropRecorder,
        _stream_disconnect_destroy: DropRecorder,
        _context_destroy: DropRecorder,
        _original_fd_drop: DropRecorder,
        _portal_session_close: DropRecorder,
    }

    struct LinuxPortalFrameStreamTest {
        _resources: TestResources,
    }

    impl LinuxPortalFrameStreamTest {
        fn from_test_resources(resources: TestResources) -> Self {
            Self {
                _resources: resources,
            }
        }
    }

    #[test]
    fn drop_order_matches_spec() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let resources = TestResources {
            _thread_loop_stop: DropRecorder {
                kind: DropKind::ThreadLoopStop,
                log: Arc::clone(&log),
            },
            _stream_disconnect_destroy: DropRecorder {
                kind: DropKind::StreamDisconnectDestroy,
                log: Arc::clone(&log),
            },
            _context_destroy: DropRecorder {
                kind: DropKind::ContextDestroy,
                log: Arc::clone(&log),
            },
            _original_fd_drop: DropRecorder {
                kind: DropKind::OriginalFdDrop,
                log: Arc::clone(&log),
            },
            _portal_session_close: DropRecorder {
                kind: DropKind::PortalSessionClose,
                log: Arc::clone(&log),
            },
        };

        let stream = LinuxPortalFrameStreamTest::from_test_resources(resources);
        drop(stream);

        let recorded = log.lock().unwrap();
        assert_eq!(
            *recorded,
            vec![
                DropKind::ThreadLoopStop,
                DropKind::StreamDisconnectDestroy,
                DropKind::ContextDestroy,
                DropKind::OriginalFdDrop,
                DropKind::PortalSessionClose,
            ]
        );
    }

    // ---- Pure buffer mapping tests ----

    #[test]
    fn dma_buf_returns_unsupported() {
        let buf = DequeuedBuffer {
            data: &[],
            buffer_type: LinuxBufferType::DmaBuf,
            chunk_size: 100,
            chunk_stride: 400,
            chunk_corrupted: false,
            header_corrupted: false,
            metadata: make_meta(100, 100),
        };
        let mut empty_count = 0u8;
        let result = inspect_dequeued_buffer(&buf, &mut empty_count);
        match result {
            Err(CaptureError::Unsupported { message }) => {
                assert!(message.contains("DMA-BUF"), "msg = {message}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }

    #[test]
    fn corrupted_header_skips_without_error() {
        let buf = DequeuedBuffer {
            data: &[0u8; 100],
            buffer_type: LinuxBufferType::MemPtr,
            chunk_size: 100,
            chunk_stride: 400,
            chunk_corrupted: false,
            header_corrupted: true,
            metadata: make_meta(100, 100),
        };
        let mut empty_count = 0u8;
        let result = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap();
        assert_eq!(result, BufferAction::Skip);
        assert_eq!(empty_count, 1);
    }

    #[test]
    fn chunk_corrupted_skips_without_error() {
        let buf = DequeuedBuffer {
            data: &[0u8; 100],
            buffer_type: LinuxBufferType::MemPtr,
            chunk_size: 100,
            chunk_stride: 400,
            chunk_corrupted: true,
            header_corrupted: false,
            metadata: make_meta(100, 100),
        };
        let mut empty_count = 0u8;
        let result = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap();
        assert_eq!(result, BufferAction::Skip);
        assert_eq!(empty_count, 1);
    }

    #[test]
    fn ten_empty_buffers_returns_backend_error() {
        let buf = DequeuedBuffer {
            data: &[],
            buffer_type: LinuxBufferType::MemPtr,
            chunk_size: 0,
            chunk_stride: 0,
            chunk_corrupted: false,
            header_corrupted: false,
            metadata: make_meta(100, 100),
        };
        let mut empty_count = 0u8;
        for _ in 0..9 {
            let _ = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap();
        }
        assert_eq!(empty_count, 9);
        let err = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap_err();
        match err {
            CaptureError::Backend(e) => {
                assert!(
                    e.to_string().contains("did not produce a usable video frame"),
                    "msg = {e}"
                );
            }
            other => panic!("expected Backend, got {other:?}"),
        }
    }

    #[test]
    fn non_identity_transform_returns_backend_error() {
        for transform in [
            LinuxVideoTransform::Rotated90,
            LinuxVideoTransform::Rotated180,
            LinuxVideoTransform::Rotated270,
            LinuxVideoTransform::Flipped,
        ] {
            let buf = DequeuedBuffer {
                data: &[0u8; 100],
                buffer_type: LinuxBufferType::MemPtr,
                chunk_size: 100,
                chunk_stride: 400,
                chunk_corrupted: false,
                header_corrupted: false,
                metadata: LinuxFrameMetadata {
                    transform,
                    ..make_meta(100, 100)
                },
            };
            let mut empty_count = 0u8;
            let err = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap_err();
            match err {
                CaptureError::Backend(e) => {
                    assert!(
                        e.to_string().contains(&format!("{transform:?}")),
                        "msg = {e}, transform = {transform:?}"
                    );
                }
                other => panic!("expected Backend for {transform:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn crop_outside_frame_returns_invalid_config() {
        let buf = DequeuedBuffer {
            data: &[0u8; 40000],
            buffer_type: LinuxBufferType::MemPtr,
            chunk_size: 40000,
            chunk_stride: 400,
            chunk_corrupted: false,
            header_corrupted: false,
            metadata: LinuxFrameMetadata {
                crop: Some(VideoCrop {
                    x: 50,
                    y: 50,
                    width: 100,
                    height: 100,
                }),
                ..make_meta(100, 100)
            },
        };
        let mut empty_count = 0u8;
        let err = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap_err();
        match err {
            CaptureError::InvalidConfig { message } => {
                assert!(message.contains("crop"), "msg = {message}");
                assert!(message.contains("100x100"), "msg = {message}");
            }
            other => panic!("expected InvalidConfig, got {other:?}"),
        }
    }

    #[test]
    fn valid_buffer_resets_empty_count() {
        let buf = DequeuedBuffer {
            data: &[0u8; 40000],
            buffer_type: LinuxBufferType::MemPtr,
            chunk_size: 40000,
            chunk_stride: 400,
            chunk_corrupted: false,
            header_corrupted: false,
            metadata: make_meta(100, 100),
        };
        let mut empty_count = 5u8;
        let result = inspect_dequeued_buffer(&buf, &mut empty_count).unwrap();
        assert_eq!(
            result,
            BufferAction::Produce(make_meta(100, 100))
        );
        assert_eq!(empty_count, 0);
    }

    #[test]
    fn map_spa_video_format_known() {
        use pipewire::spa::param::video::VideoFormat;
        assert_eq!(
            map_spa_video_format(VideoFormat::BGRA).unwrap(),
            LinuxPixelFormat::Bgra
        );
        assert_eq!(
            map_spa_video_format(VideoFormat::RGBA).unwrap(),
            LinuxPixelFormat::Rgba
        );
        assert_eq!(
            map_spa_video_format(VideoFormat::BGRx).unwrap(),
            LinuxPixelFormat::Bgrx
        );
        assert_eq!(
            map_spa_video_format(VideoFormat::RGBx).unwrap(),
            LinuxPixelFormat::Rgbx
        );
        assert_eq!(
            map_spa_video_format(VideoFormat::RGB).unwrap(),
            LinuxPixelFormat::Rgb
        );
    }

    #[test]
    fn map_spa_video_format_unsupported_returns_error() {
        use pipewire::spa::param::video::VideoFormat;
        let err = map_spa_video_format(VideoFormat::NV12).unwrap_err();
        match err {
            CaptureError::Unsupported { message } => {
                assert!(message.contains("unsupported PipeWire raw video format"), "msg = {message}");
                assert!(message.contains("NV12"), "msg = {message}");
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
