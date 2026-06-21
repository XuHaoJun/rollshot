#![doc = "Hardened rquickjs executor for rollshot-automation."]

mod lockdown;

pub use lockdown::LockedContext;

#[derive(Debug, Default)]
pub struct QuickJsExecutor;
