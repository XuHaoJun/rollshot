#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAlgorithm {
    Template,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StitchConfig {
    pub algorithm: MatchAlgorithm,
    pub min_overlap: u32,
    pub min_append: u32,
    pub accept_diff: f32,
    pub match_width: u32,
    pub duplicate_threshold: f32,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            algorithm: MatchAlgorithm::Template,
            min_overlap: 64,
            min_append: 8,
            accept_diff: 0.15,
            match_width: 512,
            duplicate_threshold: 0.01,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StitchStats {
    pub frame_count: u32,
    pub total_height: u32,
    pub last_append: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetEstimate {
    pub dy: i32,
    pub confidence: f32,
    pub method: MatchAlgorithm,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StitchOutcome {
    FirstFrame,
    Appended { added: u32 },
    NoProgress,
    NoMatch { confidence: f32 },
    Duplicate,
}

#[cfg(test)]
mod tests {
    use super::{MatchAlgorithm, StitchConfig, StitchOutcome};

    #[test]
    fn default_config_uses_template_matching() {
        let config = StitchConfig::default();

        assert_eq!(config.algorithm, MatchAlgorithm::Template);
        assert_eq!(config.min_overlap, 64);
        assert_eq!(config.min_append, 8);
        assert_eq!(config.match_width, 512);
        assert_eq!(config.duplicate_threshold, 0.01);
    }

    #[test]
    fn stitch_outcome_distinguishes_variants() {
        let appended = StitchOutcome::Appended { added: 12 };
        let no_match = StitchOutcome::NoMatch { confidence: 0.42 };

        assert_ne!(appended, StitchOutcome::FirstFrame);
        assert_ne!(no_match, StitchOutcome::Duplicate);
        assert_ne!(no_match, StitchOutcome::NoProgress);
    }
}
