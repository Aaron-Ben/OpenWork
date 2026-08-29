use std::{collections::BTreeMap, path::PathBuf};

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
    pub session_file: PathBuf,
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
        let config_root = self.state_root.join("opencode-config").join(&assignment.id);
        let opencode_config = config_root.join("opencode");
        for directory in [
            &self.state_root,
            &root,
            &bin,
            &memory,
            &notes,
            &workspace,
            &config_root,
            &opencode_config,
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
        atomic_write(
            &opencode_config.join("opencode.json"),
            br#"{"permission":{"*":"allow"}}"#,
            0o600,
        )
        .await?;

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
                "XDG_CONFIG_HOME".to_string(),
                config_root.to_string_lossy().into_owned(),
            ),
            (
                "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
                "1".to_string(),
            ),
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
            session_file: self
                .state_root
                .join("sessions")
                .join(format!("{}.session", assignment.id)),
            environment,
            token_file,
        })
    }
}

impl AgentHome {
    pub async fn save_runtime_token(&self, token: &str) -> Result<(), HomeError> {
        atomic_write(&self.token_file, token.as_bytes(), 0o600).await
    }

    pub async fn load_session(&self) -> Result<Option<String>, HomeError> {
        match tokio::fs::read_to_string(&self.session_file).await {
            Ok(session) if !session.trim().is_empty() => Ok(Some(session.trim().to_string())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn save_session(&self, session_id: &str) -> Result<(), HomeError> {
        let parent = self.session_file.parent().expect("session has parent");
        secure_directory(parent).await?;
        atomic_write(&self.session_file, session_id.as_bytes(), 0o600).await
    }
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
        "# Identity\n\n{} (`{}`)\n\n{}\n\n# Collaboration contract\n\nUse the `openwork` CLI for every collaboration action. Assistant text alone is not published.\n\n# Local workspace\n\nUse memory/, notes/, and workspace/ for local durable work.\n\n# CLI discovery\n\nRun `openwork --help` when needed.\n\n# Memory discipline\n\nNever write credentials or runtime tokens into memory.\n",
        assignment.display_name, assignment.id, assignment.system_prompt
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
