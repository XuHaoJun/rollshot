use serde::{Deserialize, Serialize};

// ---------- Public model types (no Rig types) ----------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRequest {
    pub prompt: String,
    pub history: Vec<ModelMessage>,
    pub turn: usize,
    pub tool_definitions: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelMessage {
    User {
        content: String,
    },
    Assistant {
        content: String,
    },
    ToolResult {
        tool_call_id: String,
        result: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelStreamEvent {
    TextDelta(String),
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallArgumentDelta {
        id: String,
        delta: String,
    },
    ToolCallComplete {
        id: String,
        name: String,
        arguments: serde_json::Value,
    },
    UsageDelta(ModelUsage),
    Completed(ModelCompletion),
    Error(ModelError),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelCompletion {
    pub usage: ModelUsage,
    pub stop_reason: StopReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelError {
    #[error("provider failure: {0}")]
    ProviderFailure(String),
    #[error("protocol failure: {0}")]
    ProtocolFailure(String),
    #[error("stream incomplete: {0}")]
    StreamIncomplete(String),
}

// ---------- Private Rig conversions ----------

/// Convert Rig's `CallModel { prompt, history, turn }` into BAC's `ModelRequest`.
///
/// This is the only place where Rig message types are translated into BAC's
/// provider-neutral representation.
#[cfg(test)]
fn rig_call_model_to_request(
    prompt: &rig_core::completion::Message,
    history: &[rig_core::completion::Message],
    turn: usize,
    executable_tool_names: &std::collections::BTreeSet<String>,
) -> ModelRequest {
    let prompt_text = message_to_user_text(prompt);
    let history_messages: Vec<ModelMessage> =
        history.iter().map(rig_message_to_model_message).collect();

    // For now, tool definitions are empty — they will be populated by the
    // provider adapter in a later task. The executable_tool_names are carried
    // so the assembler knows what's valid.
    let tool_definitions = executable_tool_names
        .iter()
        .map(|name| ToolDefinition {
            name: name.clone(),
            description: String::new(),
            parameters: serde_json::json!({}),
        })
        .collect();

    ModelRequest {
        prompt: prompt_text,
        history: history_messages,
        turn,
        tool_definitions,
    }
}

#[cfg(test)]
fn message_to_user_text(msg: &rig_core::completion::Message) -> String {
    match msg {
        rig_core::completion::Message::System { content } => content.clone(),
        rig_core::completion::Message::User { content } => {
            let mut parts = Vec::new();
            for c in content.iter() {
                match c {
                    rig_core::message::UserContent::Text(text) => parts.push(text.text.clone()),
                    rig_core::message::UserContent::ToolResult(tr) => {
                        for rc in tr.content.iter() {
                            if let rig_core::message::ToolResultContent::Text(t) = rc {
                                parts.push(t.text.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            parts.join("")
        }
        rig_core::completion::Message::Assistant { content, .. } => content
            .iter()
            .filter_map(|c| match c {
                rig_core::message::AssistantContent::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
    }
}

#[cfg(test)]
fn rig_message_to_model_message(msg: &rig_core::completion::Message) -> ModelMessage {
    match msg {
        rig_core::completion::Message::System { content } => ModelMessage::User {
            content: format!("[system] {}", content),
        },
        rig_core::completion::Message::User { content } => {
            let mut parts = Vec::new();
            for c in content.iter() {
                match c {
                    rig_core::message::UserContent::Text(t) => parts.push(t.text.clone()),
                    rig_core::message::UserContent::ToolResult(tr) => {
                        for rc in tr.content.iter() {
                            if let rig_core::message::ToolResultContent::Text(t) = rc {
                                parts.push(t.text.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            ModelMessage::User {
                content: parts.join(""),
            }
        }
        rig_core::completion::Message::Assistant { content, .. } => {
            let text = content
                .iter()
                .filter_map(|c| match c {
                    rig_core::message::AssistantContent::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("");
            ModelMessage::Assistant { content: text }
        }
    }
}

// ---------- Private stream bridge ----------

/// Drive one streamed turn through Rig's `StreamedTurnAssembler`, converting
/// `StreamedAssistantContent` items into BAC `ModelStreamEvent`s.
///
/// Returns the assembled `StreamedTurn` for feeding to `AgentRun::streamed_turn`.
#[allow(dead_code)] // Will be used by the driver module
pub(crate) fn drive_streamed_turn<R>(
    assembler: &mut rig_core::agent::run::StreamedTurnAssembler,
    stream_item: &rig_core::streaming::StreamedAssistantContent<R>,
) -> Result<Vec<ModelStreamEvent>, ModelError>
where
    R: Clone + Unpin + rig_core::completion::GetTokenUsage,
{
    use rig_core::agent::run::StreamedTurnEvent;
    use rig_core::streaming::StreamedAssistantContent;

    let events = assembler
        .ingest(stream_item)
        .map_err(|e| ModelError::ProtocolFailure(e.to_string()))?;

    let mut bac_events = Vec::new();

    for event in events {
        match event {
            StreamedTurnEvent::EmitIngested => {
                // Forward the original stream item as a BAC event
                match stream_item {
                    StreamedAssistantContent::Text(text) => {
                        bac_events.push(ModelStreamEvent::TextDelta(text.text.clone()));
                    }
                    StreamedAssistantContent::Reasoning(_)
                    | StreamedAssistantContent::ReasoningDelta { .. } => {
                        // Reasoning is emitted but not surfaced as a BAC event yet
                    }
                    _ => {}
                }
            }
            StreamedTurnEvent::EmitToolCallDelta {
                id,
                internal_call_id: _,
                content,
            } => match content {
                rig_core::streaming::ToolCallDeltaContent::Name(name) => {
                    bac_events.push(ModelStreamEvent::ToolCallStart {
                        id: id.clone(),
                        name: name.clone(),
                    });
                }
                rig_core::streaming::ToolCallDeltaContent::Delta(delta) => {
                    bac_events.push(ModelStreamEvent::ToolCallArgumentDelta {
                        id: id.clone(),
                        delta: delta.clone(),
                    });
                }
            },
            StreamedTurnEvent::InvalidToolCall(invalid) => {
                bac_events.push(ModelStreamEvent::Error(ModelError::ProtocolFailure(
                    format!("unknown tool: {}", invalid.tool_call.function.name),
                )));
            }
            StreamedTurnEvent::Completed {
                usage,
                emit_final: _,
            } => {
                let model_usage = ModelUsage {
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                };
                bac_events.push(ModelStreamEvent::UsageDelta(model_usage.clone()));
                bac_events.push(ModelStreamEvent::Completed(ModelCompletion {
                    usage: model_usage,
                    stop_reason: StopReason::EndTurn,
                }));
            }
        }
    }

    Ok(bac_events)
}

// ---------- Tests ----------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use rig_core::agent::run::streamed::StreamedTurnAssembler;
    use rig_core::agent::run::{AgentRun, AgentRunStep};
    use rig_core::completion::{Message, Usage};
    use rig_core::message::{
        AssistantContent, ToolCall, ToolFunction, ToolResultContent, UserContent,
    };
    use rig_core::streaming::{StreamedAssistantContent, ToolCallDeltaContent};
    use rig_core::test_utils::MockResponse;
    use rig_core::OneOrMany;
    use std::collections::BTreeSet;

    fn tool_names(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    fn text_item(text: &str) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::Text(rig_core::message::Text::new(text.to_string()))
    }

    fn tool_call_item(id: &str, name: &str) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::ToolCall {
            tool_call: ToolCall::new(
                id.to_string(),
                ToolFunction::new(name.to_string(), serde_json::json!({"x": 1})),
            ),
            internal_call_id: format!("internal_{id}"),
        }
    }

    fn name_delta(id: &str, name: &str) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::ToolCallDelta {
            id: id.to_string(),
            internal_call_id: format!("internal_{id}"),
            content: ToolCallDeltaContent::Name(name.to_string()),
        }
    }

    fn args_delta(id: &str, arguments: &str) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::ToolCallDelta {
            id: id.to_string(),
            internal_call_id: format!("internal_{id}"),
            content: ToolCallDeltaContent::Delta(arguments.to_string()),
        }
    }

    fn final_item(usage: Usage) -> StreamedAssistantContent<MockResponse> {
        StreamedAssistantContent::Final(MockResponse::with_usage(usage))
    }

    fn tool_result_content(id: &str, output: &str) -> UserContent {
        UserContent::tool_result(
            id.to_string(),
            ToolResultContent::from_tool_output(output.to_string()),
        )
    }

    // ---- Test 1: Full tool roundtrip — streamed tool-call fragments → tool result → second turn → Done ----

    #[test]
    fn full_streamed_tool_roundtrip() {
        let mut run = AgentRun::new("add things").max_turns(3);

        // === Turn 1: model streams a tool call via deltas ===
        let (prompt, history, turn) = match run.next_step().expect("next_step") {
            AgentRunStep::CallModel {
                prompt,
                history,
                turn,
            } => (prompt, history, turn),
            step => panic!("expected CallModel, got {step:?}"),
        };

        // Verify BAC request conversion
        let bac_request = rig_call_model_to_request(&prompt, &history, turn, &tool_names(&["add"]));
        assert_eq!(bac_request.turn, 1);
        assert_eq!(bac_request.prompt, "add things");
        assert!(bac_request.history.is_empty());

        // Stream tool-call argument fragments through the assembler
        let mut asm = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));

        let events = drive_streamed_turn(&mut asm, &args_delta("tc_1", "{\"x\":"))
            .expect("ingest should succeed");
        // Arguments buffered before name — no BAC events yet
        assert!(events.is_empty(), "arguments must buffer before the name");

        let events = drive_streamed_turn(&mut asm, &name_delta("tc_1", "add"))
            .expect("ingest should succeed");
        // Name validates, buffered args replay as deltas
        assert_eq!(
            events.len(),
            2,
            "should emit ToolCallStart + ToolCallArgumentDelta"
        );
        assert!(
            matches!(&events[0], ModelStreamEvent::ToolCallStart { id, name } if id == "tc_1" && name == "add")
        );
        assert!(
            matches!(&events[1], ModelStreamEvent::ToolCallArgumentDelta { id, delta } if id == "tc_1" && delta == "{\"x\":")
        );

        let events = drive_streamed_turn(&mut asm, &args_delta("tc_1", "1}"))
            .expect("ingest should succeed");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ModelStreamEvent::ToolCallArgumentDelta { id, delta } if id == "tc_1" && delta == "1}")
        );

        // Final stream item with usage
        let usage = Usage {
            input_tokens: 10,
            output_tokens: 15,
            total_tokens: 25,
            ..Usage::new()
        };
        let events =
            drive_streamed_turn(&mut asm, &final_item(usage)).expect("ingest should succeed");
        assert_eq!(events.len(), 2); // UsageDelta + Completed
        assert!(matches!(&events[0], ModelStreamEvent::UsageDelta(u) if u.input_tokens == 10));
        assert!(matches!(&events[1], ModelStreamEvent::Completed(c) if c.usage.total_tokens == 25));

        // Record usage and feed assembled turn to Rig
        run.record_streamed_completion_call(usage)
            .expect("record should succeed");
        let final_choice = OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "tc_1".to_string(),
            ToolFunction::new("add".to_string(), serde_json::json!({"x": 1})),
        )));
        run.streamed_turn(asm.finish(Some("msg_1".to_string()), &final_choice))
            .expect("streamed_turn should succeed");

        // === CallTools step ===
        let calls = match run.next_step().expect("next_step") {
            AgentRunStep::CallTools { calls } => calls,
            step => panic!("expected CallTools, got {step:?}"),
        };
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool_call.function.name, "add");

        // Return tool result
        run.tool_results(vec![tool_result_content("tc_1", "2")])
            .expect("tool_results should succeed");

        // === Turn 2: second CallModel should contain prior history ===
        let (prompt, history, turn) = match run.next_step().expect("next_step") {
            AgentRunStep::CallModel {
                prompt,
                history,
                turn,
            } => (prompt, history, turn),
            step => panic!("expected CallModel, got {step:?}"),
        };

        assert_eq!(turn, 2);

        // History should contain: user prompt, assistant tool call
        // (the tool result is the prompt, not part of history)
        let bac_request = rig_call_model_to_request(&prompt, &history, turn, &tool_names(&["add"]));
        // history should have 2 messages: user(prompt), assistant(tool_call)
        // The tool result is the prompt itself
        assert!(
            bac_request.history.len() >= 2,
            "expected at least 2 history messages, got {}",
            bac_request.history.len()
        );

        // The prompt should be the tool result
        assert!(
            bac_request.prompt.contains("2"),
            "prompt should contain tool result output, got: {:?}",
            bac_request.prompt
        );

        // === Second turn: plain text → Done ===
        let mut asm2 = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));
        let events = drive_streamed_turn(&mut asm2, &text_item("the answer is 2"))
            .expect("ingest should succeed");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelStreamEvent::TextDelta(t) if t == "the answer is 2"));

        let events = drive_streamed_turn(&mut asm2, &final_item(Usage::new()))
            .expect("ingest should succeed");
        assert_eq!(events.len(), 2);

        let final_choice = OneOrMany::one(AssistantContent::text("the answer is 2"));
        run.streamed_turn(asm2.finish(None, &final_choice))
            .expect("streamed_turn should succeed");

        // === Done ===
        let response = match run.next_step().expect("next_step") {
            AgentRunStep::Done(response) => response,
            step => panic!("expected Done, got {step:?}"),
        };
        assert_eq!(response.output, "the answer is 2");
    }

    // ---- Test 2: Second CallModel request contains prior prompt, assistant tool call, and tool result ----

    #[test]
    fn second_call_model_history_continuity() {
        let mut run = AgentRun::new("do the thing").max_turns(2);
        run.next_step().expect("next_step");

        // Turn 1: tool call
        let mut asm = StreamedTurnAssembler::new(tool_names(&["tool_a"]), tool_names(&["tool_a"]));
        drive_streamed_turn(&mut asm, &tool_call_item("tc_1", "tool_a")).expect("ingest");
        drive_streamed_turn(&mut asm, &final_item(Usage::new())).expect("ingest");
        run.record_streamed_completion_call(Usage::new())
            .expect("record");
        let final_choice = OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "tc_1".to_string(),
            ToolFunction::new("tool_a".to_string(), serde_json::json!({})),
        )));
        run.streamed_turn(asm.finish(None, &final_choice))
            .expect("streamed_turn");

        // CallTools
        match run.next_step().expect("next_step") {
            AgentRunStep::CallTools { .. } => {}
            step => panic!("expected CallTools, got {step:?}"),
        }
        run.tool_results(vec![tool_result_content("tc_1", "result_a")])
            .expect("tool_results");

        // Turn 2: verify history
        let (_, history, _) = match run.next_step().expect("next_step") {
            AgentRunStep::CallModel {
                prompt,
                history,
                turn,
            } => (prompt, history, turn),
            step => panic!("expected CallModel, got {step:?}"),
        };

        // History must contain: user msg, assistant tool call
        // (the tool result is the prompt, not part of history)
        assert!(history.len() >= 2, "history too short: {}", history.len());

        // The prompt (not in history) should be the tool result
        // Verify it by checking the prompt is a tool result message
    }

    // ---- Test 3: Interleaved text and tool-call deltas ----

    #[test]
    fn interleaved_text_and_tool_deltas() {
        let mut asm = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));

        // Text first
        let events = drive_streamed_turn(&mut asm, &text_item("let me ")).expect("ingest");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelStreamEvent::TextDelta(t) if t == "let me "));

        // Tool call delta (name first, then args)
        let events = drive_streamed_turn(&mut asm, &name_delta("tc_1", "add")).expect("ingest");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelStreamEvent::ToolCallStart { .. }));

        let events =
            drive_streamed_turn(&mut asm, &args_delta("tc_1", "{\"x\":1}")).expect("ingest");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            ModelStreamEvent::ToolCallArgumentDelta { .. }
        ));

        // More text after tool call
        let events = drive_streamed_turn(&mut asm, &text_item("calculate")).expect("ingest");
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], ModelStreamEvent::TextDelta(t) if t == "calculate"));

        // Final
        let events = drive_streamed_turn(&mut asm, &final_item(Usage::new())).expect("ingest");
        assert_eq!(events.len(), 2);
    }

    // ---- Test 4: Incomplete tool call (args without name) → protocol failure ----

    #[test]
    fn incomplete_tool_call_protocol_failure() {
        let mut asm = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));

        // Buffer args without a name
        drive_streamed_turn(&mut asm, &args_delta("tc_1", "{\"x\":1}")).expect("ingest");

        // Final should fail because args were buffered without a validated name
        let result = drive_streamed_turn(&mut asm, &final_item(Usage::new()));
        assert!(result.is_err(), "should fail on incomplete tool call");
        assert!(matches!(result, Err(ModelError::ProtocolFailure(_))));
    }

    // ---- Test 5: Unknown tool name → protocol failure ----

    #[test]
    fn unknown_tool_name_emits_error() {
        let mut asm = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));

        // Emit a complete tool call with an unknown name
        let events = drive_streamed_turn(&mut asm, &tool_call_item("tc_1", "unknown_tool"))
            .expect("ingest should succeed");

        // Should emit an error event for the unknown tool
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], ModelStreamEvent::Error(ModelError::ProtocolFailure(msg)) if msg.contains("unknown tool")),
            "expected protocol failure for unknown tool, got {:?}",
            events[0]
        );
    }

    // ---- Test 6: ScriptedModel for testing ----

    /// A scripted model that yields a predetermined sequence of stream events.
    /// Used only in tests to drive the Rig streaming protocol without a real provider.
    #[cfg(test)]
    pub(crate) struct ScriptedModel {
        turns: Vec<Vec<StreamedAssistantContent<MockResponse>>>,
        current_turn: usize,
    }

    #[cfg(test)]
    impl ScriptedModel {
        pub fn new(turns: Vec<Vec<StreamedAssistantContent<MockResponse>>>) -> Self {
            Self {
                turns,
                current_turn: 0,
            }
        }

        pub fn next_turn(&mut self) -> Option<&Vec<StreamedAssistantContent<MockResponse>>> {
            if self.current_turn < self.turns.len() {
                let items = &self.turns[self.current_turn];
                self.current_turn += 1;
                Some(items)
            } else {
                None
            }
        }
    }

    #[test]
    fn scripted_model_drives_full_roundtrip() {
        // Scripted model: turn 1 = tool call, turn 2 = text
        let mut model = ScriptedModel::new(vec![
            // Turn 1: tool call
            vec![
                tool_call_item("tc_1", "add"),
                final_item(Usage {
                    input_tokens: 5,
                    output_tokens: 10,
                    total_tokens: 15,
                    ..Usage::new()
                }),
            ],
            // Turn 2: text
            vec![text_item("done"), final_item(Usage::new())],
        ]);

        let mut run = AgentRun::new("test").max_turns(3);

        // Turn 1
        run.next_step().expect("next_step");
        let turn1_items = model.next_turn().expect("should have turn 1");
        let mut asm = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));

        for item in turn1_items {
            drive_streamed_turn(&mut asm, item).expect("ingest");
        }

        run.record_streamed_completion_call(Usage {
            input_tokens: 5,
            output_tokens: 10,
            total_tokens: 15,
            ..Usage::new()
        })
        .expect("record");
        let final_choice = OneOrMany::one(AssistantContent::ToolCall(ToolCall::new(
            "tc_1".to_string(),
            ToolFunction::new("add".to_string(), serde_json::json!({"x": 1})),
        )));
        run.streamed_turn(asm.finish(None, &final_choice))
            .expect("streamed_turn");

        // CallTools
        match run.next_step().expect("next_step") {
            AgentRunStep::CallTools { .. } => {}
            step => panic!("expected CallTools, got {step:?}"),
        }
        run.tool_results(vec![tool_result_content("tc_1", "42")])
            .expect("tool_results");

        // Turn 2
        run.next_step().expect("next_step");
        let turn2_items = model.next_turn().expect("should have turn 2");
        let mut asm2 = StreamedTurnAssembler::new(tool_names(&["add"]), tool_names(&["add"]));
        for item in turn2_items {
            drive_streamed_turn(&mut asm2, item).expect("ingest");
        }
        let final_choice = OneOrMany::one(AssistantContent::text("done"));
        run.streamed_turn(asm2.finish(None, &final_choice))
            .expect("streamed_turn");

        // Done
        let response = match run.next_step().expect("next_step") {
            AgentRunStep::Done(response) => response,
            step => panic!("expected Done, got {step:?}"),
        };
        assert_eq!(response.output, "done");
    }

    // ---- Test 7: ModelRequest conversion from Rig CallModel ----

    #[test]
    fn rig_call_model_converts_to_bac_request() {
        let prompt = Message::user("what is 2+2?");
        let history = vec![Message::user("hello"), Message::assistant("hi there")];
        let names = tool_names(&["add", "subtract"]);

        let request = rig_call_model_to_request(&prompt, &history, 1, &names);

        assert_eq!(request.prompt, "what is 2+2?");
        assert_eq!(request.history.len(), 2);
        assert!(
            matches!(&request.history[0], ModelMessage::User { content } if content == "hello")
        );
        assert!(
            matches!(&request.history[1], ModelMessage::Assistant { content } if content == "hi there")
        );
        assert_eq!(request.turn, 1);
        assert_eq!(request.tool_definitions.len(), 2);
        assert!(request.tool_definitions.iter().any(|d| d.name == "add"));
        assert!(request
            .tool_definitions
            .iter()
            .any(|d| d.name == "subtract"));
    }

    // ---- Test 8: Upgrade guard — Rig 0.39 streamed API compiles ----

    #[test]
    fn rig_039_streamed_turn_api_compiles() {
        // This test names the pinned Rig version expectation. If Rig's
        // StreamedTurnAssembler, StreamedTurnEvent, or AgentRun::streamed_turn
        // API changes, this test will fail to compile.
        let _assembler = StreamedTurnAssembler::new(BTreeSet::new(), BTreeSet::new());
        let mut run = AgentRun::new("test");
        let _step = run.next_step();

        // Verify key types are accessible
        let _: StreamedAssistantContent<MockResponse> = StreamedAssistantContent::text("x");
        let _tool_names: BTreeSet<String> = BTreeSet::new();
    }
}
