use crate::types::{CaptureOptions, CaptureProbe, CapturedFrame};

pub trait CaptureBackend {
    fn name(&self) -> &'static str;
    fn probe(&self) -> CaptureProbe;
    fn start(&mut self, options: CaptureOptions) -> Result<Box<dyn FrameStream>, String>;
}

pub trait FrameStream: Send {
    fn next_frame(&mut self) -> Result<CapturedFrame, String>;
}

#[derive(Debug, Clone)]
pub struct FakeFrameStream {
    frames: Vec<CapturedFrame>,
    index: usize,
}

impl FakeFrameStream {
    pub fn new(frames: Vec<CapturedFrame>) -> Self {
        Self { frames, index: 0 }
    }
}

impl FrameStream for FakeFrameStream {
    fn next_frame(&mut self) -> Result<CapturedFrame, String> {
        let frame = self
            .frames
            .get(self.index)
            .cloned()
            .ok_or_else(|| String::from("end of fake stream"))?;

        self.index += 1;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::{FakeFrameStream, FrameStream};
    use crate::types::{CapturedFrame, FrameMetadata, Size};
    use std::time::SystemTime;

    #[test]
    fn fake_stream_returns_frames_in_order() {
        let first = CapturedFrame {
            pixels: vec![255, 0, 0, 255],
            size: Size {
                width: 1,
                height: 1,
            },
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        };
        let second = CapturedFrame {
            pixels: vec![0, 255, 0, 255],
            size: Size {
                width: 1,
                height: 1,
            },
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        };
        let mut stream = FakeFrameStream::new(vec![first.clone(), second.clone()]);

        assert_eq!(stream.next_frame().expect("first frame"), first);
        assert_eq!(stream.next_frame().expect("second frame"), second);
        assert_eq!(
            stream.next_frame().expect_err("end of stream"),
            "end of fake stream"
        );
    }
}
