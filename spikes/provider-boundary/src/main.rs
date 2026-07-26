#[cfg(all(feature = "rig-039", feature = "rig-040"))]
compile_error!("enable exactly one of rig-039 or rig-040");
#[cfg(not(any(feature = "rig-039", feature = "rig-040")))]
compile_error!("enable exactly one of rig-039 or rig-040");

#[cfg(feature = "rig-039")]
use rig_core_039 as rig;
#[cfg(feature = "rig-040")]
use rig_core_040 as rig;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Fixture {
    chunks: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Observation {
    Text { text: String },
    ToolCall { id: String, name: String },
    Final { total_tokens: u64 },
    Error { category: String },
    End,
}

fn fixture(name: &str) -> Fixture {
    let all: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/cases.json"
    ))
    .expect("fixture JSON must parse");
    serde_json::from_value(all[name].clone()).expect("named fixture must parse")
}

fn main() {
    eprintln!("completion probe is added in Task 2");
}
