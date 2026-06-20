// spike-visual-diff/src/main.rs
//
// Visual-diff prototype for the Smart Redaction Agent Workbench (spec §8.2 / §13.4).
// Renders, over a tall test image in a scrollable+Canvas:
//   1. Accepted annotations in the existing style (filled opaque rect).
//   2. Proposed candidates in a VISUALLY DISTINCT style (dashed outline + low-opacity fill).
//   3. Before/after TOGGLE — candidates hidden vs shown.
//   4. A `similar`-based SOURCE-DIFF text pane (old vs new automation JS).
//   5. A Workflow IR SEMANTIC-SUMMARY pane (hand-authored IR for sample detector).
//
// Compile evidence only (no display available). Run: cargo build.
//
// Iced 0.14 patterns (see iced-rs skill):
// - `iced::application(new, update, view)` single-window entry point.
// - `canvas::Program<Message>` for custom 2D drawing.
// - `scrollable` + `canvas` for the tall image area.
// - `column` / `row` for the side panels.

use iced::widget::canvas::{Cache, Frame, Geometry, Path, Program, Stroke, Text as CanvasText};
use iced::widget::{button, canvas, column, container, row, scrollable, text};
use iced::{Color, Element, Fill, Point, Rectangle, Renderer, Size, Task, Theme};
use similar::{ChangeTag, TextDiff};

// ---------------------------------------------------------------------------
// Geometry helpers (mirrors ImageRect pattern from rollshot-app)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
}

// ---------------------------------------------------------------------------
// Annotation kinds
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct AcceptedAnnotation {
    bounds: Rect,
    label: String,
}

#[derive(Debug, Clone)]
struct Candidate {
    bounds: Rect,
    confidence: f32, // 0..1; low (<0.5) gets extra low-opacity treatment
    label: String,
}

// ---------------------------------------------------------------------------
// Hand-authored Workflow IR (spec §8.3 / §13.4)
// ---------------------------------------------------------------------------

struct WorkflowIr {
    detector_name: &'static str,
    capabilities: &'static [&'static str],
    thresholds: &'static [(&'static str, &'static str)],
    candidate_count_before: usize,
    candidate_count_after: usize,
}

const SAMPLE_IR: WorkflowIr = WorkflowIr {
    detector_name: "valid_detector.js",
    capabilities: &["ocr", "layout-analysis"],
    thresholds: &[("confidence", "> 0.8"), ("min_area_px", "> 400")],
    candidate_count_before: 3,
    candidate_count_after: 7,
};

fn ir_summary(ir: &WorkflowIr) -> String {
    let caps = ir.capabilities.join(", ");
    let thresholds = ir
        .thresholds
        .iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let delta = ir.candidate_count_after as i64 - ir.candidate_count_before as i64;
    let delta_str = if delta >= 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    };
    format!(
        "Detector: {}\nCapabilities: {}\nThresholds: {}\nCandidates: {} → {} ({delta_str})",
        ir.detector_name,
        caps,
        thresholds,
        ir.candidate_count_before,
        ir.candidate_count_after,
    )
}

// ---------------------------------------------------------------------------
// Source diff helper
// ---------------------------------------------------------------------------

const OLD_JS: &str = r#"// v1 detector
function detect(img) {
  const threshold = 0.7;
  return ocr(img).filter(r => r.confidence > threshold);
}
"#;

const NEW_JS: &str = r#"// v2 detector
function detect(img) {
  const threshold = 0.8;
  const min_area = 400;
  return ocr(img)
    .filter(r => r.confidence > threshold)
    .filter(r => r.area > min_area);
}
"#;

fn compute_diff_lines(old: &str, new: &str) -> Vec<(ChangeTag, String)> {
    let diff = TextDiff::from_lines(old, new);
    diff.iter_all_changes()
        .map(|c| (c.tag(), c.to_string_lossy().into_owned()))
        .collect()
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum OverlayMode {
    Before, // only accepted annotations shown
    After,  // accepted + proposed candidates
}

struct App {
    /// Image dimensions (simulated tall image: 1920×4800 px)
    image_size: Size,
    accepted: Vec<AcceptedAnnotation>,
    candidates: Vec<Candidate>,
    overlay_mode: OverlayMode,
    canvas_cache: Cache,
    diff_lines: Vec<(ChangeTag, String)>,
    ir_text: String,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let image_size = Size::new(1920.0, 4800.0);

        // Accepted annotations — existing committed redactions
        let accepted = vec![
            AcceptedAnnotation {
                bounds: Rect { x: 200.0, y: 150.0, w: 400.0, h: 30.0 },
                label: "SSN".into(),
            },
            AcceptedAnnotation {
                bounds: Rect { x: 100.0, y: 600.0, w: 280.0, h: 26.0 },
                label: "Name".into(),
            },
            AcceptedAnnotation {
                bounds: Rect { x: 500.0, y: 2200.0, w: 360.0, h: 30.0 },
                label: "DOB".into(),
            },
        ];

        // Proposed candidates from the agent (spec §8.2)
        let candidates = vec![
            Candidate {
                bounds: Rect { x: 150.0, y: 900.0, w: 320.0, h: 28.0 },
                confidence: 0.92,
                label: "Email".into(),
            },
            Candidate {
                bounds: Rect { x: 300.0, y: 1400.0, w: 200.0, h: 26.0 },
                confidence: 0.45, // low-confidence
                label: "Phone (low conf)".into(),
            },
            Candidate {
                bounds: Rect { x: 80.0, y: 3100.0, w: 440.0, h: 30.0 },
                confidence: 0.88,
                label: "Address".into(),
            },
            Candidate {
                bounds: Rect { x: 200.0, y: 4000.0, w: 260.0, h: 28.0 },
                confidence: 0.71,
                label: "Account #".into(),
            },
        ];

        let diff_lines = compute_diff_lines(OLD_JS, NEW_JS);
        let ir_text = ir_summary(&SAMPLE_IR);

        (
            Self {
                image_size,
                accepted,
                candidates,
                overlay_mode: OverlayMode::Before,
                canvas_cache: Cache::default(),
                diff_lines,
                ir_text,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleOverlay => {
                self.overlay_mode = match self.overlay_mode {
                    OverlayMode::Before => OverlayMode::After,
                    OverlayMode::After => OverlayMode::Before,
                };
                self.canvas_cache.clear();
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        // ---- image + annotation canvas (left/main pane) ----
        let scale = 0.5_f32; // fit 1920px image into ~960px display width
        let display_w = self.image_size.width * scale;
        let display_h = self.image_size.height * scale;

        let canvas_widget = canvas(AnnotationCanvas {
            image_size: self.image_size,
            accepted: &self.accepted,
            candidates: &self.candidates,
            overlay_mode: self.overlay_mode,
            scale,
            cache: &self.canvas_cache,
        })
        .width(display_w)
        .height(display_h);

        let toggle_label = match self.overlay_mode {
            OverlayMode::Before => "Show Candidates (After)",
            OverlayMode::After => "Hide Candidates (Before)",
        };

        let image_pane = column![
            button(toggle_label).on_press(Message::ToggleOverlay),
            scrollable(
                container(canvas_widget)
                    .width(Fill)
                    .padding(8)
            )
            .height(Fill)
        ]
        .spacing(8)
        .width(Fill);

        // ---- source diff pane (right, top) ----
        let diff_lines_widgets: Vec<Element<'_, Message>> = self
            .diff_lines
            .iter()
            .map(|(tag, line)| {
                let prefix = match tag {
                    ChangeTag::Equal => " ",
                    ChangeTag::Insert => "+",
                    ChangeTag::Delete => "-",
                };
                let color = match tag {
                    ChangeTag::Equal => Color::from_rgb(0.8, 0.8, 0.8),
                    ChangeTag::Insert => Color::from_rgb(0.4, 0.9, 0.4),
                    ChangeTag::Delete => Color::from_rgb(0.9, 0.4, 0.4),
                };
                text(format!("{prefix} {}", line.trim_end()))
                    .color(color)
                    .size(12)
                    .into()
            })
            .collect();

        let diff_pane = container(
            column![
                text("Source Diff").size(14),
                scrollable(
                    column(diff_lines_widgets).spacing(1)
                )
                .height(200)
            ]
            .spacing(6),
        )
        .padding(8)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.15))),
            border: iced::Border {
                color: Color::from_rgb(0.3, 0.3, 0.5),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        // ---- Workflow IR semantic summary pane (right, bottom) ----
        let ir_pane = container(
            column![
                text("Workflow IR Summary").size(14),
                text(self.ir_text.as_str()).size(12).color(Color::from_rgb(0.85, 0.85, 0.6))
            ]
            .spacing(6),
        )
        .padding(8)
        .style(|_theme: &Theme| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.08, 0.12, 0.08))),
            border: iced::Border {
                color: Color::from_rgb(0.3, 0.5, 0.3),
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        });

        // ---- side panel ----
        let side_panel = column![diff_pane, ir_pane]
            .spacing(12)
            .width(400);

        row![image_pane, side_panel].spacing(12).padding(12).into()
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Message {
    ToggleOverlay,
}

// ---------------------------------------------------------------------------
// Canvas program — draws accepted annotations + (optionally) proposed candidates
// ---------------------------------------------------------------------------

struct AnnotationCanvas<'a> {
    image_size: Size,
    accepted: &'a [AcceptedAnnotation],
    candidates: &'a [Candidate],
    overlay_mode: OverlayMode,
    scale: f32,
    cache: &'a Cache,
}

impl<'a> Program<Message> for AnnotationCanvas<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let s = self.scale;
        let img_w = self.image_size.width;
        let img_h = self.image_size.height;

        let geo = self.cache.draw(renderer, bounds.size(), |frame: &mut Frame| {
            // Image background (simulated — no actual image loaded)
            frame.fill_rectangle(
                Point::ORIGIN,
                Size::new(img_w * s, img_h * s),
                Color::from_rgb(0.18, 0.18, 0.22),
            );

            // Simulated text content lines across the tall image
            let line_spacing = 40.0_f32 * s;
            let mut y = 20.0_f32 * s;
            while y < img_h * s {
                frame.fill_rectangle(
                    Point::new(40.0 * s, y),
                    Size::new(img_w * s * 0.85, 12.0 * s),
                    Color::from_rgb(0.35, 0.35, 0.40),
                );
                y += line_spacing;
            }

            // Visible region for culling (viewport = 600px of image height at scale)
            let viewport_rect = Rect {
                x: 0.0,
                y: 0.0,
                w: img_w,
                h: (600.0 / s).min(img_h),
            };

            // Draw accepted annotations (existing style: opaque filled red rect)
            for ann in self.accepted {
                if !ann.bounds.intersects(&viewport_rect) {
                    continue; // frustum cull — skip off-screen in a real scroll scenario
                }
                frame.fill_rectangle(
                    Point::new(ann.bounds.x * s, ann.bounds.y * s),
                    Size::new(ann.bounds.w * s, ann.bounds.h * s),
                    Color::from_rgba8(180, 30, 30, 220.0 / 255.0),
                );
                frame.fill_text(CanvasText {
                    content: ann.label.clone(),
                    position: Point::new((ann.bounds.x + 4.0) * s, (ann.bounds.y + 2.0) * s),
                    color: Color::WHITE,
                    size: iced::Pixels(11.0 * s),
                    ..CanvasText::default()
                });
            }

            // Draw proposed candidates (only in After mode) — spec §8.2 distinct style
            if self.overlay_mode == OverlayMode::After {
                for cand in self.candidates {
                    // Opacity: low-confidence (<0.5) gets extra transparency
                    let base_alpha = if cand.confidence < 0.5 { 0.25 } else { 0.45 };
                    let fill_color =
                        Color::from_rgba(0.2, 0.6, 1.0, base_alpha);

                    // Fill with dashed-style outline (iced canvas doesn't support
                    // actual dashes; use a thin outline stroke instead)
                    frame.fill_rectangle(
                        Point::new(cand.bounds.x * s, cand.bounds.y * s),
                        Size::new(cand.bounds.w * s, cand.bounds.h * s),
                        fill_color,
                    );

                    let outline_path = Path::rectangle(
                        Point::new(cand.bounds.x * s, cand.bounds.y * s),
                        Size::new(cand.bounds.w * s, cand.bounds.h * s),
                    );
                    let outline_color = if cand.confidence < 0.5 {
                        Color::from_rgba(0.4, 0.7, 1.0, 0.5) // pale blue for low-conf
                    } else {
                        Color::from_rgba(0.2, 0.6, 1.0, 0.9) // solid blue for high-conf
                    };
                    frame.stroke(
                        &outline_path,
                        Stroke::default()
                            .with_color(outline_color)
                            .with_width(if cand.confidence < 0.5 { 1.0 } else { 2.0 }),
                    );

                    // Confidence label
                    let conf_pct = (cand.confidence * 100.0) as u32;
                    frame.fill_text(CanvasText {
                        content: format!("{} {}%", cand.label, conf_pct),
                        position: Point::new(
                            (cand.bounds.x + 4.0) * s,
                            (cand.bounds.y + 2.0) * s,
                        ),
                        color: Color::from_rgba(1.0, 1.0, 1.0, 0.9),
                        size: iced::Pixels(10.0 * s),
                        ..CanvasText::default()
                    });
                }
            }
        });

        vec![geo]
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title(|_: &App| String::from("Visual Diff Spike"))
        .theme(App::theme)
        .run()
}
