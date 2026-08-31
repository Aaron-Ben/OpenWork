use std::{collections::BTreeMap, path::PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::protocol::AgentAssignment;

pub struct HomeManager {
    state_root: PathBuf,
    shim_executable: PathBuf,
    runtime_base_url: String,
}

pub struct AgentHome {
    pub root: PathBuf,
    pub triage_root: PathBuf,
    pub config_root: PathBuf,
    pub state_file: PathBuf,
    pub config_fingerprint: String,
    pub environment: BTreeMap<String, String>,
    token_file: PathBuf,
}

impl HomeManager {
    pub fn new(state_root: PathBuf, shim_executable: PathBuf, runtime_base_url: String) -> Self {
        Self {
            state_root,
            shim_executable,
            runtime_base_url,
        }
    }

    pub async fn materialize(
        &self,
        assignment: &AgentAssignment,
        runtime_token: &str,
    ) -> Result<AgentHome, HomeError> {
        if !valid_agent_id(&assignment.id) {
            return Err(HomeError::InvalidAgentId(assignment.id.clone()));
        }
        let root = self.state_root.join("agents").join(&assignment.id);
        let bin = root.join("bin");
        let memory = root.join("memory");
        let notes = root.join("notes");
        let workspace = root.join("workspace");
        let triage_root = self.state_root.join("triage");
        let config_root = self
            .state_root
            .join(format!("{}-config", assignment.engine_id))
            .join(&assignment.id);
        for directory in [
            &self.state_root,
            &root,
            &bin,
            &memory,
            &notes,
            &workspace,
            &triage_root,
            &config_root,
        ] {
            secure_directory(directory).await?;
        }

        atomic_write(
            &root.join("AGENTS.md"),
            standing_prompt(assignment).as_bytes(),
            0o600,
        )
        .await?;
        atomic_write(
            &root.join(".openwork-standing-prompt.md"),
            standing_prompt(assignment).as_bytes(),
            0o600,
        )
        .await?;
        let memory_file = memory.join("MEMORY.md");
        if !tokio::fs::try_exists(&memory_file).await? {
            atomic_write(
                &memory_file,
                b"# Memory\n\nDurable notes for this agent. Never store secrets here.\n",
                0o600,
            )
            .await?;
        }
        let token_file = bin.join(".runtime-token");
        atomic_write(&token_file, runtime_token.as_bytes(), 0o600).await?;
        let shim = bin.join("openwork");
        if tokio::fs::symlink_metadata(&shim).await.is_ok() {
            tokio::fs::remove_file(&shim).await?;
        }
        tokio::fs::symlink(&self.shim_executable, &shim).await?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        let original_data_home = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|home| format!("{home}/.local/share"))
        });
        let mut environment = BTreeMap::from([
            ("HOME".to_string(), root.to_string_lossy().into_owned()),
            (
                "PATH".to_string(),
                format!("{}:{original_path}", bin.to_string_lossy()),
            ),
            (
                "OPENWORK_RUNTIME_BASE_URL".to_string(),
                self.runtime_base_url.clone(),
            ),
            (
                "OPENWORK_RUNTIME_TOKEN_FILE".to_string(),
                token_file.to_string_lossy().into_owned(),
            ),
            (
                "OPENWORK_AGENT_HOME".to_string(),
                root.to_string_lossy().into_owned(),
            ),
        ]);
        if let Some(data_home) = original_data_home {
            environment.insert("XDG_DATA_HOME".to_string(), data_home);
        }
        Ok(AgentHome {
            root,
            triage_root,
            config_root,
            state_file: self
                .state_root
                .join("sessions")
                .join(format!("{}.session", assignment.id)),
            config_fingerprint: persona_hash(assignment),
            environment,
            token_file,
        })
    }
}

impl AgentHome {
    pub async fn save_runtime_token(&self, token: &str) -> Result<(), HomeError> {
        atomic_write(&self.token_file, token.as_bytes(), 0o600).await
    }
}

fn persona_hash(assignment: &AgentAssignment) -> String {
    let fields = [
        assignment.id.as_str(),
        assignment.display_name.as_str(),
        assignment.role.as_deref().unwrap_or_default(),
        assignment.bio.as_deref().unwrap_or_default(),
        assignment.system_prompt.as_str(),
    ];
    let mut hasher = Sha256::new();
    for field in fields {
        let normalized = field.replace("\r\n", "\n");
        let normalized = normalized.trim();
        hasher.update(normalized.len().to_be_bytes());
        hasher.update(normalized.as_bytes());
    }
    format!("sha256:{:x}", hasher.finalize())
}

async fn secure_directory(path: &std::path::Path) -> Result<(), std::io::Error> {
    tokio::fs::create_dir_all(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(())
}

async fn atomic_write(path: &std::path::Path, bytes: &[u8], mode: u32) -> Result<(), HomeError> {
    let parent = path.parent().expect("managed file has parent");
    secure_directory(parent).await?;
    let temporary = parent.join(format!(".openwork-{}.tmp", Uuid::new_v4().simple()));
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(mode))
            .await?;
    }
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}

fn standing_prompt(assignment: &AgentAssignment) -> String {
    format!(
        "# Identity\n\n{} (`{}`)\n\nRole: {}\n\nBio: {}\n\n{}\n\n# Collaboration contract\n\nUse the `openwork` CLI for every collaboration action. Assistant text alone is not published.\n\n## Glance and yield\n\n- A human may address one named teammate by name or role without @-mentioning them. If you are that teammate, answer; otherwise stay out. A message to the whole group may be answered by the group.\n- Reply from the actual posted messages, never from an imagined queue position. Use `openwork glance <room-id>` when you need to reread the room.\n- Post optimistically. If `openwork reply` returns HELD, read the newer messages, reconsider, and retry with the provided token only if your revised reply is still needed.\n- Do not repeat a peer. If another Agent already covered your point, react or stay silent. Stop when the task is complete.\n- Do not claim a chat turn or reserve a conversational slot. Claims are only for genuine shared work that another teammate could duplicate.\n\n# Local workspace\n\nUse memory/, notes/, and workspace/ for local durable work.\n\n# CLI discovery\n\nRun `openwork --help` when needed.\n\n# Memory discipline\n\nNever write credentials or runtime tokens into memory.\n",
        assignment.display_name,
        assignment.id,
        assignment.role.as_deref().unwrap_or("unspecified"),
        assignment.bio.as_deref().unwrap_or("unspecified"),
        assignment.system_prompt
    )
}

fn valid_agent_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && id.len() <= 48
        && chars.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

#[derive(Debug, thiserror::Error)]
pub enum HomeError {
    #[error("invalid Agent id: {0}")]
    InvalidAgentId(String),
    #[error("Agent home I/O failed: {0}")]
    Io(#[from] std::io::Error),
}
