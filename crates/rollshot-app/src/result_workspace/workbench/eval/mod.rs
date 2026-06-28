//! Phase D Smart Redaction evaluation harness (test-only).
//!
//! Deterministic gate over synthetic-image fixtures, scored two ways:
//! full-loop cassette replay (layer1) and extracted golden-source geometry
//! scoring (layer2). See `docs/smart-redaction-eval.md`.

pub(crate) mod cassette;
pub(crate) mod fixture;
pub(crate) mod layer2;
pub(crate) mod render;
pub(crate) mod scoring;
