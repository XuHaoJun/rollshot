//! Stable explicit tracing targets for the Action Guide engine. Diagnostics
//! record only capability, source category, counts, and lifecycle outcomes —
//! never key values, typed text, click coordinates, frame contents, or paths.

pub(crate) const TARGET_ACTION: &str = "rollshot::action";
pub(crate) const TARGET_EXPORT: &str = "rollshot::action::export";
