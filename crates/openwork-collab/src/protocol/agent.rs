use serde::{Deserialize, Serialize};

use super::{BoardView, CardView, MessageView, ParticipantView, RoomView};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriagePayload {
    pub verdict: Option<TriageVerdict>,
    pub instructions: Option<String>,
    pub input: Option<String>,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriageVerdict {
    pub actionable: bool,
    pub reason: String,
    pub prompt_note: String,
    pub source: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriageReportRequest {
    pub run_id: String,
    pub verdict: TriageVerdict,
    pub model: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InboxResponse {
    pub trigger: Option<TriggerEnvelope>,
    pub messages: Vec<MessageView>,
    pub climates: Vec<ClimateView>,
    pub carried_over: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClimateView {
    pub agent_id: String,
    pub about_participant_id: String,
    pub affinity: f64,
    pub trust: f64,
    pub last_note: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TriggerEnvelope {
    pub dispatch_id: String,
    pub agent_id: String,
    pub runtime_session_id: String,
    pub trigger: String,
    pub deliveries: Vec<DeliveryRange>,
    pub agenda_focus: Option<AgendaFocus>,
    pub carried_over: bool,
    pub issued_at: i64,
    pub expires_at: i64,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaFocus {
    pub room_id: Option<String>,
    pub card_id: Option<String>,
    pub room_sequence: Option<i64>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgendaCandidate {
    AssignedCard {
        candidate_id: String,
        card_id: String,
        room_id: Option<String>,
        title: String,
        column: String,
        assignment: String,
        updated_at: i64,
        room_sequence: Option<i64>,
        recent_context: Vec<MessageView>,
    },
    StalledRoom {
        candidate_id: String,
        room_id: String,
        last_sequence: i64,
        last_activity_at: i64,
        open_cards: Vec<String>,
        recent_context: Vec<MessageView>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaCandidateSet {
    pub id: String,
    pub agent_id: String,
    pub runtime_session_id: String,
    pub candidates: Vec<AgendaCandidate>,
    pub issued_at: i64,
    pub expires_at: i64,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaPayload {
    pub candidate_set: AgendaCandidateSet,
    pub classify_prompt: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "decision",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgendaDecision {
    Act {
        candidate_id: String,
        reason: String,
    },
    Decline {
        reason: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaDecisionRequest {
    pub candidate_set: AgendaCandidateSet,
    pub decision: AgendaDecision,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub latency_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaDecisionResponse {
    pub trigger: Option<TriggerEnvelope>,
    pub focused_brief: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRange {
    pub room_id: String,
    pub from_seq: i64,
    pub up_to_seq: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpenRunRequest {
    pub trigger: TriggerEnvelope,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub id: String,
    pub status: String,
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FinishRunRequest {
    pub status: String,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub assistant_text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommandRequest {
    pub request_id: String,
    pub command: AgentCommand,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentCommand {
    Inbox,
    Rooms,
    Messages {
        room_id: String,
        tail: u32,
    },
    Members {
        room_id: String,
    },
    Participants,
    Glance {
        room_id: String,
    },
    Reply {
        room_id: String,
        body: String,
        held_token: Option<String>,
    },
    Ack {
        room_id: String,
    },
    DirectMessage {
        participant_id: String,
        body: String,
    },
    ClimateShow {
        participant_id: Option<String>,
    },
    ClimateNote {
        participant_id: String,
        affinity: f64,
        trust: f64,
        note: String,
    },
    BoardList,
    CardList {
        board_id: Option<String>,
    },
    CardCreate {
        board_id: String,
        column_id: String,
        title: String,
        description: Option<String>,
        assignee_id: Option<String>,
    },
    CardClaim {
        card_id: String,
    },
    CardMove {
        card_id: String,
        column_id: String,
        before_card_id: Option<String>,
    },
}

impl AgentCommand {
    pub fn is_mutating(&self) -> bool {
        !matches!(
            self,
            Self::Inbox
                | Self::Rooms
                | Self::Messages { .. }
                | Self::Members { .. }
                | Self::Participants
                | Self::Glance { .. }
                | Self::ClimateShow { .. }
                | Self::BoardList
                | Self::CardList { .. }
        )
    }

    pub fn requires_active_run(&self) -> bool {
        !matches!(
            self,
            Self::Rooms
                | Self::Messages { .. }
                | Self::Members { .. }
                | Self::Participants
                | Self::ClimateShow { .. }
                | Self::BoardList
                | Self::CardList { .. }
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentCommandResponse {
    pub result: AgentCommandResult,
    pub effects: Vec<AgentCommandEffect>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentCommandResult {
    Inbox {
        carried_over: bool,
        messages: Vec<MessageView>,
    },
    Glance {
        room_id: String,
        compose_anchor: i64,
        members: Vec<ParticipantView>,
        messages: Vec<MessageView>,
    },
    Rooms {
        rooms: Vec<RoomView>,
    },
    Messages {
        room_id: String,
        messages: Vec<MessageView>,
    },
    Members {
        room_id: String,
        members: Vec<ParticipantView>,
    },
    Participants {
        participants: Vec<ParticipantView>,
    },
    Climates {
        climates: Vec<ClimateView>,
    },
    Climate {
        climate: ClimateView,
    },
    MessagePublished {
        message: MessageView,
    },
    Acknowledged {
        room_id: String,
        up_to_seq: i64,
    },
    DirectMessageSent {
        room_id: String,
        message: MessageView,
    },
    Boards {
        boards: Vec<BoardView>,
    },
    Cards {
        cards: Vec<CardView>,
    },
    Card {
        card: CardView,
    },
    Held {
        room_id: String,
        retry_token: String,
        messages: Vec<MessageView>,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentCommandEffect {
    MessagePublished {
        room_id: String,
        message_id: String,
        sequence: i64,
    },
    InboxAcknowledged {
        room_id: String,
        up_to_seq: i64,
    },
    DirectRoomOpened {
        room_id: String,
        participant_id: String,
    },
    CardCreated {
        board_id: String,
        card_id: String,
    },
    CardAssigned {
        card_id: String,
        assignee_id: String,
    },
    CardMoved {
        card_id: String,
        column_id: String,
        position: i32,
    },
    ClimateUpdated {
        about_participant_id: String,
    },
}
