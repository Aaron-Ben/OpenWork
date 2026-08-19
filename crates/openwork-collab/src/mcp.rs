use std::{
    collections::HashMap,
    io,
    sync::{Arc, RwLock},
};

use axum::Router;
use rmcp::{
    RoleServer,
    handler::server::wrapper::Parameters,
    schemars,
    service::RequestContext,
    tool, tool_router,
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde::Deserialize;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    coordination::CoordinationHub,
    event::{CollabEventKind, CollabEventPublisher},
    model::AgentReplyOutcome,
    storage::CollabStorage,
};

const TOKEN_HEADER: &str = "x-openwork-token";

#[derive(Debug, Clone)]
pub struct MessageNotice {
    pub room_id: String,
    pub author_id: String,
    pub body: String,
    pub sequence: i64,
}

#[derive(Debug, Clone, Default)]
pub struct IdentityRegistry {
    tokens: Arc<RwLock<HashMap<String, String>>>,
}

impl IdentityRegistry {
    pub fn issue(&self, agent_id: &str) -> Result<String, String> {
        let token = format!("owc_{}", Uuid::new_v4().simple());
        self.tokens
            .write()
            .map_err(|_| "identity registry lock is poisoned".to_string())?
            .insert(token.clone(), agent_id.to_string());
        Ok(token)
    }

    pub fn resolve(&self, token: &str) -> Result<Option<String>, String> {
        Ok(self
            .tokens
            .read()
            .map_err(|_| "identity registry lock is poisoned".to_string())?
            .get(token)
            .cloned())
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReplyRequest {
    #[schemars(description = "Room id to post into")]
    room_id: String,
    #[schemars(description = "Exact message body to publish")]
    body: String,
    #[serde(default)]
    #[schemars(description = "HELD token returned by a previous rejected reply")]
    held_token: Option<String>,
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
struct InboxRequest {}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GlanceRequest {
    #[schemars(description = "Room id to reread")]
    room_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReactRequest {
    #[schemars(description = "Message id to react to")]
    message_id: String,
    #[schemars(description = "Exact emoji to attach")]
    emoji: String,
}

#[derive(Clone)]
struct CollaborationMcp {
    storage: CollabStorage,
    identities: IdentityRegistry,
    notices: mpsc::UnboundedSender<MessageNotice>,
    coordination: CoordinationHub,
    events: CollabEventPublisher,
}

#[tool_router(server_handler)]
impl CollaborationMcp {
    #[tool(
        description = "Publish a message in an OpenWork room. Your identity comes from the authenticated MCP connection."
    )]
    async fn reply(
        &self,
        Parameters(request): Parameters<ReplyRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        let agent_id = self.authenticated_agent(&context)?;
        if !self
            .storage
            .is_member(&request.room_id, &agent_id)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err(format!(
                "Agent {agent_id} is not a member of room {}",
                request.room_id
            ));
        }
        let seen_sequence = self
            .coordination
            .effective_seen(&agent_id, &request.room_id, request.held_token.as_deref())
            .await;
        let outcome = self
            .storage
            .send_agent_reply(&request.room_id, &agent_id, &request.body, seen_sequence)
            .await
            .map_err(|error| error.to_string())?;
        match outcome {
            AgentReplyOutcome::Published(outcome) => {
                if !outcome.deduplicated {
                    let _ = self.notices.send(MessageNotice {
                        room_id: request.room_id,
                        author_id: agent_id,
                        body: request.body,
                        sequence: outcome.message.sequence,
                    });
                }
                serde_json::to_string(&AgentReplyOutcome::Published(outcome))
                    .map_err(|error| error.to_string())
            }
            AgentReplyOutcome::Held(held) => {
                let token = self
                    .coordination
                    .issue_held(&agent_id, &request.room_id, held.peer_sequence)
                    .await;
                self.events
                    .publish(CollabEventKind::ReplyHeld {
                        agent_id,
                        room_id: request.room_id,
                        peer_sequence: held.peer_sequence,
                    })
                    .await;
                serde_json::to_string(&serde_json::json!({
                    "status": "held",
                    "heldToken": token,
                    "peerSequence": held.peer_sequence,
                    "messages": held.messages,
                    "instruction": "Reread these published peer messages, recompute, then call openwork_reply again with held_token."
                }))
                .map_err(|error| error.to_string())
            }
        }
    }

    #[tool(
        description = "Read all of your currently unread OpenWork room messages. This tool never advances the persisted read cursor."
    )]
    async fn inbox(
        &self,
        Parameters(_request): Parameters<InboxRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        let agent_id = self.authenticated_agent(&context)?;
        let inbox = self
            .storage
            .inbox(&agent_id)
            .await
            .map_err(|error| error.to_string())?;
        serde_json::to_string(&inbox).map_err(|error| error.to_string())
    }

    #[tool(
        description = "Reread the latest published state of an OpenWork room before recomputing a HELD reply."
    )]
    async fn glance(
        &self,
        Parameters(request): Parameters<GlanceRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        let agent_id = self.authenticated_agent(&context)?;
        if !self
            .storage
            .is_member(&request.room_id, &agent_id)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err(format!(
                "Agent {agent_id} is not a member of room {}",
                request.room_id
            ));
        }
        let glance = self
            .storage
            .glance(&request.room_id, 50)
            .await
            .map_err(|error| error.to_string())?;
        self.coordination
            .observe(&agent_id, &request.room_id, glance.highest_sequence)
            .await;
        serde_json::to_string(&glance).map_err(|error| error.to_string())
    }

    #[tool(
        description = "React to a published teammate message without posting a duplicate reply."
    )]
    async fn react(
        &self,
        Parameters(request): Parameters<ReactRequest>,
        context: RequestContext<RoleServer>,
    ) -> Result<String, String> {
        let agent_id = self.authenticated_agent(&context)?;
        let reaction = self
            .storage
            .add_reaction(&request.message_id, &agent_id, &request.emoji)
            .await
            .map_err(|error| error.to_string())?;
        serde_json::to_string(&reaction).map_err(|error| error.to_string())
    }
}

impl CollaborationMcp {
    fn authenticated_agent(&self, context: &RequestContext<RoleServer>) -> Result<String, String> {
        let token = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.headers.get(TOKEN_HEADER))
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| "missing X-OpenWork-Token".to_string())?;
        self.identities
            .resolve(token)?
            .ok_or_else(|| "invalid X-OpenWork-Token".to_string())
    }
}

pub struct McpServerHandle {
    pub url: String,
    task: JoinHandle<io::Result<()>>,
}

impl McpServerHandle {
    pub async fn shutdown(self) {
        match self.task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => eprintln!("MCP server stopped with error: {error}"),
            Err(error) if error.is_cancelled() => {}
            Err(error) => eprintln!("MCP server task failed: {error}"),
        }
    }
}

pub async fn start_server(
    storage: CollabStorage,
    identities: IdentityRegistry,
    notices: mpsc::UnboundedSender<MessageNotice>,
    coordination: CoordinationHub,
    events: CollabEventPublisher,
    cancel: CancellationToken,
) -> Result<McpServerHandle, io::Error> {
    let handler = CollaborationMcp {
        storage,
        identities,
        notices,
        coordination,
        events,
    };
    let service: StreamableHttpService<CollaborationMcp, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            StreamableHttpServerConfig::default(),
        );
    let app = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(cancel.cancelled_owned())
            .await
    });
    Ok(McpServerHandle {
        url: format!("http://{address}/mcp"),
        task,
    })
}

#[cfg(test)]
mod tests {
    use super::IdentityRegistry;

    #[test]
    fn issued_token_binds_one_agent_without_accepting_an_agent_parameter() {
        let registry = IdentityRegistry::default();
        let token = registry.issue("alice").unwrap();
        assert_eq!(registry.resolve(&token).unwrap().as_deref(), Some("alice"));
        assert_eq!(registry.resolve("not-a-token").unwrap(), None);
    }
}
