use std::path::{Path, PathBuf};

use serde_json::json;
use thiserror::Error;

use crate::model::Agent;

#[derive(Debug, Clone)]
pub struct HomeManager {
    root: PathBuf,
    mcp_url: String,
}

impl HomeManager {
    pub fn new(root: impl Into<PathBuf>, mcp_url: impl Into<String>) -> Self {
        Self {
            root: root.into(),
            mcp_url: mcp_url.into(),
        }
    }

    pub fn agent_home(&self, agent_id: &str) -> PathBuf {
        self.root.join("homes").join(agent_id)
    }

    pub async fn repair(&self, agent: &Agent, token: &str) -> Result<PathBuf, HomeError> {
        let home = self.agent_home(&agent.id);
        let memory = home.join("memory");
        tokio::fs::create_dir_all(&memory).await?;

        let description = match (&agent.role, &agent.bio) {
            (Some(role), Some(bio)) => format!("{role}: {bio}"),
            (Some(role), None) => role.clone(),
            (None, Some(bio)) => bio.clone(),
            (None, None) => format!("OpenWork collaborator {}", agent.display_name),
        };
        let config = json!({
            "$schema": "https://opencode.ai/config.json",
            "default_agent": agent.id,
            "agent": {
                agent.id.clone(): {
                    "description": description,
                    "mode": "primary",
                    "model": format!("{}/{}", agent.provider_id, agent.model_id),
                    "prompt": agent.system_prompt,
                    "permission": {},
                }
            },
            "mcp": {
                "openwork": {
                    "type": "remote",
                    "url": self.mcp_url,
                    "enabled": true,
                    "headers": {"X-OpenWork-Token": token},
                    "oauth": false,
                    "timeout": 10000
                }
            }
        });
        write_managed(
            &home.join("opencode.json"),
            &serde_json::to_vec_pretty(&config)?,
        )
        .await?;
        write_managed(&home.join("AGENTS.md"), render_agents_md(agent).as_bytes()).await?;
        let memory_file = memory.join("MEMORY.md");
        if !tokio::fs::try_exists(&memory_file).await? {
            tokio::fs::write(&memory_file, b"# Memory\n").await?;
        }
        Ok(home)
    }
}

fn render_agents_md(agent: &Agent) -> String {
    format!(
        "# Identity\n\n{name}\n\n{prompt}\n\n\
         # OpenWork collaboration protocol\n\n\
         You are a persistent peer in shared rooms. Read pending room messages with \
         `openwork_inbox`; a response counts only after `openwork_reply` succeeds. \
         The daemon binds your identity from the MCP token, so never claim another identity. \
         Stay inside this home directory unless the user explicitly approves access.\n\n\
         ## Addressing teammates\n\n\
         Each wake delivers a roster of the current room members. Address and mention \
         teammates by their roster `id` (for example `alice`), never by display name: \
         display names may be duplicated or non-ASCII, while the id is the exact string \
         that names them in room history and in `@mentions`. The roster is delivered \
         per wake and is not reproduced here, because membership changes.\n\n\
         ## Five coordination rules\n\n\
         1. When a human names a teammate, check who was named; if it was not you, stay quiet or use `openwork_react`.\n\
         2. Reply from real published state, never from assumptions about your place in a queue.\n\
         3. Send optimistically; the server is the safety net. Do not loop on `openwork_glance`; after HELD, reread, recompute, and resend.\n\
         4. Do not repeat what a teammate already said; stop after speaking.\n\
         5. Do not claim chat turns. Claims are only for real shared work on a board card.\n\n\
         Put work teammates and users should see on the shared board; use session todo only for steps in your current turn.\n",
        name = agent.display_name,
        prompt = agent.system_prompt.trim(),
    )
}

async fn write_managed(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("managed");
    let temporary = path.with_file_name(format!(".{file_name}.openwork-tmp"));
    tokio::fs::write(&temporary, contents).await?;
    tokio::fs::rename(temporary, path).await
}

#[derive(Debug, Error)]
pub enum HomeError {
    #[error("failed to repair an Agent home: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to render opencode.json: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> Agent {
        Agent {
            id: "alice".to_string(),
            display_name: "Alice".to_string(),
            role: None,
            bio: None,
            system_prompt: "Be precise.".to_string(),
            provider_id: "opencode".to_string(),
            model_id: "main".to_string(),
            opencode_session_id: None,
            enabled: true,
            scanner_enabled: false,
        }
    }

    #[test]
    fn teaches_addressing_teammates_by_roster_id() {
        let rendered = render_agents_md(&agent());
        assert!(rendered.contains("Address and mention"));
        assert!(rendered.contains("roster `id`"));
        assert!(rendered.contains("`alice`"));
    }

    #[test]
    fn does_not_embed_the_roster_itself() {
        let rendered = render_agents_md(&agent());
        // The roster ships per wake; a baked-in copy would go stale with
        // membership. Member display names or JSON keys must not appear.
        assert!(!rendered.contains("displayName"));
        assert!(!rendered.contains("\"members\""));
    }

    #[test]
    fn keeps_the_user_written_persona_verbatim() {
        let rendered = render_agents_md(&agent());
        assert!(rendered.contains("Be precise."));
        assert!(rendered.contains("Alice"));
    }
}
