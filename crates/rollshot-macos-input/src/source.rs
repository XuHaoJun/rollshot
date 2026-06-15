//! MacosInputSource CGEventTap glue (filled in Task 7).

/// Placeholder so `crates/rollshot-macos-input/src/lib.rs` can re-export the
/// type while the crate is being scaffolded. Replaced by the real
/// `SemanticInputSource` implementation in Task 7.
pub struct MacosInputSource;

impl MacosInputSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MacosInputSource {
    fn default() -> Self {
        Self::new()
    }
}
