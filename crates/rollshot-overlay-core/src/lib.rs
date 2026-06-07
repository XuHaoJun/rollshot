//! Platform-independent overlay UI logic shared between the Tauri webview
//! overlay (`rollshot-tauri-app`) and the native iced overlay (`rollshot-iced-overlay`):
//! the live-preview viewport generator and the crop visual design tokens, so
//! both render from one source of truth. No iced / Tauri / webview deps.
//!
//! Modules are introduced by the TDD tasks that create them, so this scaffold
//! stays buildable on its own.

pub mod capture_miss;
pub mod chrome_placement;
pub mod preview;
pub mod tokens;
