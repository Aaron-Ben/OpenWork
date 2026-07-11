use serde::{Deserialize, Serialize};

use super::ModelResponse;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelEvent {
    TextStart {
        index: u32,
        id: String,
    },
    TextDelta {
        index: u32,
        delta: String,
    },
    TextEnd {
        index: u32,
        id: String,
    },
    ReasoningStart {
        index: u32,
        id: String,
    },
    ReasoningDelta {
        index: u32,
        delta: String,
    },
    ReasoningEnd {
        index: u32,
        id: String,
    },
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    ToolCallDelta {
        index: u32,
        id: String,
        partial_input: String,
    },
    ToolCallEnd {
        index: u32,
        id: String,
    },
    ResponseCompleted {
        response: Box<ModelResponse>,
    },
}

pub type GenerateStreamEvent = ModelEvent;
