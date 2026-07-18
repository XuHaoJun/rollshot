//! Platform-neutral Action Guide data models. These types carry only
//! privacy-filtered semantic information: never raw key codes, typed text,
//! device names, or device paths.

/// Milliseconds since recording start. Monotonic; assigned by the recorder.
pub type Millis = u64;
/// Monotonic identifier for a retained frame within one session.
pub type FrameId = u64;
/// Monotonic identifier for a detector candidate within one session.
pub type CandidateId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRegion {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticKey {
    Enter,
    Tab,
}

/// A privacy-filtered semantic input action. Deliberately carries no raw key
/// code, no Unicode text, and no device identity — ordinary typing collapses to
/// `TypingActivity`; only Enter/Tab survive as semantic keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticAction {
    Click {
        button: MouseButton,
        position: Option<Point>,
    },
    ScrollActivity,
    TypingActivity,
    SemanticKey(SemanticKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TimedSemanticAction {
    pub action: SemanticAction,
    pub at_ms: Millis,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputSourceKind {
    LinuxEvdev,
    MacosCgEvent,
    VisualOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DegradedReason {
    /// macOS Input Monitoring denied, or Linux evdev ACL missing.
    PermissionDenied,
    /// Linux: no readable `/dev/input/event*` device.
    NoInputDevice,
    /// Source could not start (tap creation failed, no reader opened, or — in
    /// P0a — no platform semantic source is wired into the build).
    SourceStartFailed,
    /// Source started but failed mid-session (null tap, all readers died).
    RuntimeFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum InputCapability {
    SemanticEvents,
    VisualOnly { reason: DegradedReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CandidateKind {
    Click,
    Typing,
    Scroll,
    UiChanged,
}

/// Privacy-safe reason a candidate was created. Never carries coordinates,
/// key values, or text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DetectReason {
    ClickConfirmed,
    TypingSettled,
    ScrollSettled,
    VisualChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FrameRef {
    pub id: FrameId,
    pub at_ms: Millis,
}

/// A detector output: one retained candidate with its chosen keyframe and a
/// bounded, ordered set of nearby frames for replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateStep {
    pub id: CandidateId,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe: FrameId,
    pub nearby: Vec<FrameId>,
}

/// A reviewable, editable guide step. `index` is 1-based and renumbered on
/// delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuideStep {
    pub index: usize,
    pub title: String,
    pub caption: String,
    pub kind: CandidateKind,
    pub reason: DetectReason,
    pub at_ms: Millis,
    pub keyframe: FrameId,
    pub nearby: Vec<FrameId>,
    pub source: CandidateId,
}

/// Default deterministic label for a candidate kind (spec §Timeline Workspace).
pub fn default_title(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Click => "Click",
        CandidateKind::Typing => "Enter text",
        CandidateKind::Scroll => "Scroll",
        CandidateKind::UiChanged => "UI changed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_action_serde_round_trips_kebab_case() {
        let actions = [
            SemanticAction::Click {
                button: MouseButton::Left,
                position: Some(Point { x: 3, y: 4 }),
            },
            SemanticAction::Click {
                button: MouseButton::Right,
                position: None,
            },
            SemanticAction::ScrollActivity,
            SemanticAction::TypingActivity,
            SemanticAction::SemanticKey(SemanticKey::Enter),
        ];
        for a in actions {
            let json = serde_json::to_string(&a).expect("serialize");
            let back: SemanticAction = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(a, back);
        }
        // kebab-case for unit/struct variants and nested keys.
        assert_eq!(
            serde_json::to_string(&SemanticAction::ScrollActivity).unwrap(),
            "\"scroll-activity\""
        );
    }

    #[test]
    fn input_capability_serde_round_trips() {
        let cap = InputCapability::VisualOnly {
            reason: DegradedReason::PermissionDenied,
        };
        let json = serde_json::to_string(&cap).unwrap();
        assert!(json.contains("visual-only"), "json = {json}");
        assert!(json.contains("permission-denied"), "json = {json}");
        assert_eq!(serde_json::from_str::<InputCapability>(&json).unwrap(), cap);
        assert_eq!(
            serde_json::to_string(&InputCapability::SemanticEvents).unwrap(),
            "\"semantic-events\""
        );
    }

    #[test]
    fn default_titles_match_spec_labels() {
        assert_eq!(default_title(CandidateKind::Click), "Click");
        assert_eq!(default_title(CandidateKind::Typing), "Enter text");
        assert_eq!(default_title(CandidateKind::Scroll), "Scroll");
        assert_eq!(default_title(CandidateKind::UiChanged), "UI changed");
    }
}
