#![doc = "Hardened rquickjs executor for rollshot-automation."]

mod bridge;
mod execution;
mod lockdown;

pub use lockdown::LockedContext;

#[derive(Debug, Default)]
pub struct QuickJsExecutor;
