//! Product-policy validation for a proposed candidate set (spec §9.4 limits).
//! Geometric per-op validity (zero-area, non-finite, kind) is the document
//! layer's job; this layer enforces count / total-area / out-of-bounds policy.
//!
//! Area accounting is a deliberate CONSERVATIVE upper bound: each redaction's
//! raw (un-clamped) width*height is summed independently, so overlapping
//! candidates are double-counted and off-image extent is included, and the
//! resulting fraction may exceed 1.0. This never under-reports coverage (the
//! safe direction for a redaction limit); it is NOT the exact painted-pixel
//! fraction. Geometric clamping / zero-area rejection stays the document
//! layer's job (see the §6 validation split).

use rollshot_image_document::ImageRect;
use serde::{Deserialize, Serialize};

use crate::proposal::{CandidateId, ProposedCandidate, ProposedEdit};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PolicyLimits {
    pub max_candidates: u32,
    /// Total redaction area as a fraction of the image area (0.0..=1.0).
    pub max_total_area_fraction: f32,
    pub allow_out_of_bounds: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum PolicyError {
    #[error("too many candidates: {count} exceeds limit {max}")]
    TooManyCandidates { count: u32, max: u32 },
    #[error("total redaction area fraction {fraction} exceeds limit {max}")]
    ExcessiveTotalArea { fraction: f32, max: f32 },
    #[error("candidate is out of bounds")]
    OutOfBounds { candidate: CandidateId },
}

/// Return the redaction bounds a candidate contributes, if any (only redaction
/// edits have an "area" / "bounds" for policy purposes).
fn redaction_bounds(c: &ProposedCandidate) -> Option<ImageRect> {
    match &c.edit {
        ProposedEdit::AddRedaction { bounds }
        | ProposedEdit::UpdateRedactionBounds { bounds, .. } => Some(*bounds),
        _ => None,
    }
}

pub fn validate_policy(
    candidates: &[ProposedCandidate],
    limits: &PolicyLimits,
    image_dims: (u32, u32),
) -> Result<(), PolicyError> {
    let count = candidates.len() as u32;
    if count > limits.max_candidates {
        return Err(PolicyError::TooManyCandidates {
            count,
            max: limits.max_candidates,
        });
    }

    let (w, h) = image_dims;
    let image_area = (w as f32) * (h as f32);

    if !limits.allow_out_of_bounds {
        for c in candidates {
            if let Some(b) = redaction_bounds(c) {
                if b.x < 0.0 || b.y < 0.0 || b.x + b.width > w as f32 || b.y + b.height > h as f32 {
                    return Err(PolicyError::OutOfBounds { candidate: c.id });
                }
            }
        }
    }

    if image_area > 0.0 {
        let total: f32 = candidates
            .iter()
            .filter_map(redaction_bounds)
            .map(|b| b.width.max(0.0) * b.height.max(0.0))
            .sum();
        let fraction = total / image_area;
        if fraction > limits.max_total_area_fraction {
            return Err(PolicyError::ExcessiveTotalArea {
                fraction,
                max: limits.max_total_area_fraction,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CandidateId, ProposedCandidate, ProposedEdit, Provenance, ProvenanceSource};
    use rollshot_image_document::{ImagePoint, ImageRect};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> ImageRect {
        ImageRect::from_corners(ImagePoint::new(x, y), ImagePoint::new(x + w, y + h))
    }
    fn redaction(id: u64, r: ImageRect) -> ProposedCandidate {
        ProposedCandidate {
            id: CandidateId(id),
            edit: ProposedEdit::AddRedaction { bounds: r },
            confidence: 0.9,
            label: "test".into(),
            rationale: None,
            provenance: Provenance {
                source: ProvenanceSource::Agent { run_id: 1 },
            },
        }
    }
    fn limits() -> PolicyLimits {
        PolicyLimits {
            max_candidates: 3,
            max_total_area_fraction: 0.5,
            allow_out_of_bounds: false,
        }
    }

    #[test]
    fn accepts_within_all_limits() {
        let cands = vec![redaction(1, rect(0.0, 0.0, 10.0, 10.0))];
        assert!(validate_policy(&cands, &limits(), (100, 100)).is_ok());
    }

    #[test]
    fn rejects_too_many_candidates() {
        let cands: Vec<_> = (0..4)
            .map(|i| redaction(i, rect(0.0, 0.0, 2.0, 2.0)))
            .collect();
        assert!(matches!(
            validate_policy(&cands, &limits(), (100, 100)),
            Err(PolicyError::TooManyCandidates { count: 4, max: 3 })
        ));
    }

    #[test]
    fn rejects_excessive_total_area() {
        // 80x80 = 6400 over 100x100 = 10000 -> 0.64 > 0.5 limit.
        let cands = vec![redaction(1, rect(0.0, 0.0, 80.0, 80.0))];
        assert!(matches!(
            validate_policy(&cands, &limits(), (100, 100)),
            Err(PolicyError::ExcessiveTotalArea { .. })
        ));
    }

    #[test]
    fn rejects_out_of_bounds_when_disallowed() {
        let cands = vec![redaction(7, rect(90.0, 90.0, 30.0, 30.0))]; // extends past 100x100
        assert!(matches!(
            validate_policy(&cands, &limits(), (100, 100)),
            Err(PolicyError::OutOfBounds {
                candidate: CandidateId(7)
            })
        ));
    }

    #[test]
    fn allows_out_of_bounds_when_enabled() {
        let mut l = limits();
        l.allow_out_of_bounds = true;
        let cands = vec![redaction(7, rect(90.0, 90.0, 30.0, 30.0))];
        assert!(validate_policy(&cands, &l, (100, 100)).is_ok());
    }
}
