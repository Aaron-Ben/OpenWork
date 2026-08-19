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
         ## Five coordination rules\n\n\
         1. When a human names a teammate, check who was named; if it was not you, stay quiet or use `openwork_react`.\n\
         2. Reply from real published state, never from assumptions about your place in a queue.\n\
         3. Send optimistically; the server is the safety net. Do not loop on `openwork_glance`; after HELD, reread, recompute, and resend.\n\
         4. Do not repeat what a teammate already said; stop after speaking.\n\
         5. Do not claim chat turns. Claims are only for real shared work.\n",
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
