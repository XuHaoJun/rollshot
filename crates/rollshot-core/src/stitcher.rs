use image::RgbaImage;

use crate::duplicate;
use crate::image_ext::append_below;
use crate::matcher::estimate_offset;
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

        let signature = duplicate::signature(&frame);
        if let Some(prev_sig) = self.last_good_signature.as_ref() {
            if duplicate::is_duplicate(prev_sig, &signature, self.config.duplicate_threshold) {
                return StitchOutcome::Duplicate;
            }
        }

        let estimate = estimate_offset(anchor, &frame, self.last_offset, &self.config);
        if estimate.confidence > self.config.accept_diff {
            return StitchOutcome::NoMatch {
                confidence: estimate.confidence,
            };
        }

        let dy = estimate.dy.max(0) as u32;
        if dy < self.config.min_append {
            return StitchOutcome::NoProgress;
        }

        let combined = append_below(
            self.full_image
                .as_ref()
                .expect("full image present after first frame"),
            &frame,
            dy,
        );
        let total_height = combined.height();

        self.full_image = Some(combined);
        self.last_good_frame = Some(frame);
        self.last_good_signature = Some(signature);
        self.last_offset = estimate.dy;
        self.stats.frame_count += 1;
        self.stats.total_height = total_height;
        self.stats.last_append = dy;

        StitchOutcome::Appended { added: dy }
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
