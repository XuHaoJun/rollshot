//! Editor/session state. Stub — expanded in the editor-state task.
pub struct EditorState {
    #[allow(dead_code)]
    pub navigator_open: bool,
    pub saved_state_id: u64,
}

impl EditorState {
    pub fn new(saved_state_id: u64, navigator_open: bool) -> Self {
        Self { navigator_open, saved_state_id }
    }
}
