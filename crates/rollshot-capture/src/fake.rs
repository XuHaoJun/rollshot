use crate::error::CaptureError;
use crate::types::CapturedFrame;
use crate::FrameStream;

/// In-memory frame stream used by unit tests. Not reachable from the CLI.
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
    fn next_frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let frame = self
            .frames
            .get(self.index)
            .cloned()
            .ok_or(CaptureError::EndOfStream)?;
        self.index += 1;
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::FakeFrameStream;
    use crate::error::CaptureError;
    use crate::types::{CapturedFrame, FrameMetadata};
    use crate::FrameStream;
    use image::{Rgba, RgbaImage};
    use std::time::SystemTime;

    fn make_frame(color: [u8; 4]) -> CapturedFrame {
        CapturedFrame {
            image: RgbaImage::from_pixel(1, 1, Rgba(color)),
            timestamp: SystemTime::UNIX_EPOCH,
            metadata: FrameMetadata::fake(),
        }
    }

    #[test]
    fn fake_stream_returns_frames_in_order() {
        let first = make_frame([255, 0, 0, 255]);
        let second = make_frame([0, 255, 0, 255]);
        let mut stream = FakeFrameStream::new(vec![first.clone(), second.clone()]);

        let got_first = stream.next_frame().expect("first frame");
        assert_eq!(got_first.image.get_pixel(0, 0).0, [255, 0, 0, 255]);

        let got_second = stream.next_frame().expect("second frame");
        assert_eq!(got_second.image.get_pixel(0, 0).0, [0, 255, 0, 255]);

        match stream.next_frame() {
            Err(CaptureError::EndOfStream) => {}
            other => panic!("expected EndOfStream, got {other:?}"),
        }
    }
}
