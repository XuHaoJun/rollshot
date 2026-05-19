use image::RgbaImage;

use crate::duplicate;
use crate::types::{StitchConfig, StitchOutcome, StitchStats};

pub struct Stitcher {
    config: StitchConfig,
    full_image: Option<RgbaImage>,
    last_good_frame: Option<RgbaImage>,
    last_good_signature: Option<Vec<u8>>,
    last_offset: i32,
    stats: StitchStats,
}

impl Stitcher {
    pub fn new(config: StitchConfig) -> Self {
        Self {
            config,
            full_image: None,
            last_good_frame: None,
            last_good_signature: None,
            last_offset: 0,
            stats: StitchStats::default(),
        }
    }

    pub fn push_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        if self.full_image.is_none() {
            return self.accept_first_frame(frame);
        }

        let anchor = self
            .last_good_frame
            .as_ref()
            .expect("anchor present after first frame");

        if anchor.dimensions() != frame.dimensions() {
            return StitchOutcome::NoMatch {
                confidence: f32::INFINITY,
            };
        }

        let _ = &self.config;
        let _ = &self.last_offset;
        let _ = frame;
        StitchOutcome::NoMatch {
            confidence: f32::INFINITY,
        }
    }

    pub fn full_image(&self) -> Option<&RgbaImage> {
        self.full_image.as_ref()
    }

    pub fn stats(&self) -> StitchStats {
        self.stats
    }

    fn accept_first_frame(&mut self, frame: RgbaImage) -> StitchOutcome {
        let height = frame.height();
        self.stats = StitchStats {
            frame_count: 1,
            total_height: height,
            last_append: height,
        };
        self.last_good_signature = Some(duplicate::signature(&frame));
        self.last_good_frame = Some(frame.clone());
        self.full_image = Some(frame);
        StitchOutcome::FirstFrame
    }
}
