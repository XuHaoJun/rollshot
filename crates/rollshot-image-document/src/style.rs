//! First-release visual defaults (spec §6 — reviewed product deliverable).
//! All sizes are full-resolution image pixels. The UI exposes no style
//! controls; these constants are the single source of annotation appearance
//! for BOTH the live overlay and flattened output.

use crate::geometry::Rgba8;

/// Callout accent (number bubble fill, leader triangle): #E5484D.
pub const ACCENT: Rgba8 = Rgba8::new(0xE5, 0x48, 0x4D, 0xFF);
pub const WHITE: Rgba8 = Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF);

/// Number bubble radius.
pub const NUMBER_BUBBLE_RADIUS: f32 = 17.0;
/// White outline ring width around the bubble (contrast treatment so the
/// bubble reads on accent-colored content).
pub const NUMBER_BUBBLE_OUTLINE_WIDTH: f32 = 2.0;
/// Number label: bold white digits, shrink-to-fit below this size.
pub const NUMBER_FONT_PX: f32 = 20.0;
pub const NUMBER_FONT_MIN_PX: f32 = 9.0;
/// Label must fit within this multiple of the bubble radius.
pub const NUMBER_LABEL_MAX_WIDTH_FACTOR: f32 = 1.6;

/// Leader triangle half-width at its base.
pub const LEADER_HALF_WIDTH: f32 = 8.0;
/// Leader base center sits at this fraction of the radius from bubble center.
pub const LEADER_BASE_FACTOR: f32 = 0.82;
/// Below this separation (× radius) no leader is drawn — the callout is a
/// plain stamp (click-created).
pub const LEADER_MIN_SEPARATION_FACTOR: f32 = 0.45;

/// Text Note: white text on a dark backing plate for legibility over busy
/// screenshot content (square corners in the first release).
pub const TEXT_NOTE_FONT_PX: f32 = 18.0;
pub const TEXT_NOTE_TEXT_COLOR: Rgba8 = WHITE;
pub const TEXT_NOTE_PLATE: Rgba8 = Rgba8::new(0x11, 0x11, 0x11, 0xD9); // ~85% black
pub const TEXT_NOTE_PLATE_PADDING: f32 = 8.0;
/// Line height factor (matches iced's default relative line height).
pub const TEXT_LINE_HEIGHT: f32 = 1.3;

/// Opaque Redaction: solid black, replaces covered pixels (spec §9.4).
pub const REDACTION_FILL: Rgba8 = Rgba8::new(0x00, 0x00, 0x00, 0xFF);

/// Deterministic baseline fonts, vendored. cosmic-text falls back to system
/// fonts for glyphs these lack (e.g. CJK).
pub const FONT_REGULAR_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans.ttf");
pub const FONT_BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/DejaVuSans-Bold.ttf");
/// Family name consumers use to select the vendored font (e.g. iced
/// `Font::with_name`).
pub const FONT_FAMILY_NAME: &str = "DejaVu Sans";
