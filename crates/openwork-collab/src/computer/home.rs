use std::{collections::BTreeMap, path::PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::protocol::AgentAssignment;

pub struct HomeManager {
    openwork_root: PathBuf,
    runtime_root: PathBuf,
    runtime_bin: PathBuf,
    runtime_base_url: String,
}

pub struct AgentHome {
    pub work_root: PathBuf,
    pub config_root: PathBuf,
    pub state_file: PathBuf,
    pub context_fingerprint: String,
    pub environment: BTreeMap<String, String>,
    token_file: PathBuf,
}

impl HomeManager {
    pub async fn prepare(
        openwork_root: PathBuf,
        runtime_session_id: &str,
        shim_executable: PathBuf,
        runtime_base_url: String,
    ) -> Result<Self, HomeError> {
        if !valid_managed_segment(runtime_session_id) {
            return Err(HomeError::InvalidRuntimeSessionId(
                runtime_session_id.to_string(),
            ));
        }
        let runtime_root = openwork_root.join("runtime").join(runtime_session_id);
        let runtime_bin = runtime_root.join("bin");
        for directory in [
            &openwork_root,
            &openwork_root.join("agents"),
            &runtime_root,
            &runtime_bin,
            &runtime_root.join("agents"),
            &runtime_root.join("derived"),
        ] {
            secure_directory(directory).await?;
        }
        replace_symlink(&runtime_bin.join("openwork"), &shim_executable).await?;
        Ok(Self {
            openwork_root,
            runtime_root,
            runtime_bin,
            runtime_base_url,
        })
    }

    pub async fn materialize(
        &self,
        assignment: &AgentAssignment,
        runtime_token: &str,
    ) -> Result<AgentHome, HomeError> {
        if !valid_agent_id(&assignment.id) {
            return Err(HomeError::InvalidAgentId(assignment.id.clone()));
        }
        if !valid_managed_segment(&assignment.engine_id) {
            return Err(HomeError::InvalidEngineId(assignment.engine_id.clone()));
        }
        let root = self.openwork_root.join("agents").join(&assignment.id);
        let work_root = root.join("work");
        let engine_root = root.join("engines").join(&assignment.engine_id);
        let token_file = self
            .runtime_root
            .join("agents")
            .join(&assignment.id)
            .join("runtime-token");
        let config_root = self
            .runtime_root
            .join("derived")
            .join(&assignment.id)
            .join(&assignment.engine_id);
        for directory in [
            &root,
            &work_root,
            &engine_root,
            token_file.parent().expect("runtime token has a parent"),
            &config_root,
        ] {
            secure_directory(directory).await?;
        }

        let managed_context = standing_prompt(assignment);
        let context_fingerprint = managed_context_fingerprint(&managed_context);
        atomic_write(&root.join("AGENTS.md"), managed_context.as_bytes(), 0o600).await?;
        atomic_write(&token_file, runtime_token.as_bytes(), 0o600).await?;

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
                format!("{}:{original_path}", self.runtime_bin.to_string_lossy()),
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
            (
                "XDG_CACHE_HOME".to_string(),
                config_root.join("cache").to_string_lossy().into_owned(),
            ),
            (
                "XDG_STATE_HOME".to_string(),
                config_root.join("state").to_string_lossy().into_owned(),
            ),
        ]);
        if let Some(data_home) = original_data_home {
            environment.insert("XDG_DATA_HOME".to_string(), data_home);
        }
        Ok(AgentHome {
            work_root,
            config_root,
            state_file: engine_root.join("session.json"),
            context_fingerprint,
            environment,
            token_file,
        })
    }
}

async fn replace_symlink(
    path: &std::path::Path,
    target: &std::path::Path,
) -> Result<(), HomeError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            tokio::fs::remove_dir_all(path).await?;
        }
        Ok(_) => tokio::fs::remove_file(path).await?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    tokio::fs::symlink(target, path).await?;
    Ok(())
}

impl AgentHome {
    pub async fn save_runtime_token(&self, token: &str) -> Result<(), HomeError> {
        atomic_write(&self.token_file, token.as_bytes(), 0o600).await
    }
}

fn managed_context_fingerprint(managed_context: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(managed_context.as_bytes()))
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
        "# Identity\n\n{} (`{}`)\n\nRole: {}\n\n{}\n\n# Collaboration contract\n\nUse the `openwork` CLI for every collaboration action. Assistant text alone is not published.\n\n## Glance and yield\n\n- A human may address one named teammate by name or role without @-mentioning them. If you are that teammate, answer; otherwise stay out. A message to the whole group may be answered by the group.\n- Reply from the actual posted messages, never from an imagined queue position. Use `openwork glance <room-id>` when you need to reread the room.\n- Post optimistically. If `openwork reply` returns HELD, read the newer messages, reconsider, and retry with the provided token only if your revised reply is still needed.\n- Do not repeat a peer. If another Agent already covered your point, stay silent. Stop when the task is complete.\n- Do not claim a chat turn or reserve a conversational slot. Claims are only for genuine shared work that another teammate could duplicate.\n\n# Local workspace\n\nUse the local Agent workspace for durable work.\n\n# CLI discovery\n\nRun `openwork --help` when needed.\n",
        assignment.display_name,
        assignment.id,
        assignment.role.as_deref().unwrap_or("unspecified"),
        assignment.persona
    )
}

fn valid_agent_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && id.len() <= 48
        && chars.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn valid_managed_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[derive(Debug, thiserror::Error)]
pub enum HomeError {
    #[error("invalid Agent id: {0}")]
    InvalidAgentId(String),
    #[error("invalid Engine id: {0}")]
    InvalidEngineId(String),
    #[error("invalid RuntimeSession id: {0}")]
    InvalidRuntimeSessionId(String),
    #[error("Agent home I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(all(test, unix))]
mod tests {
    use sha2::{Digest, Sha256};

    use crate::protocol::AgentAssignment;

    use super::HomeManager;

    #[tokio::test]
    async fn separates_persistent_agent_home_from_session_runtime_files() {
        let directory = tempfile::tempdir().unwrap();
        let openwork_root = directory.path().join(".openwork");
        let shim = directory.path().join("desktop");
        tokio::fs::write(&shim, b"binary").await.unwrap();
        let runtime_session_id = format!("runtime-{}", "a".repeat(32));
        let manager = HomeManager::prepare(
            openwork_root.clone(),
            &runtime_session_id,
            shim.clone(),
            "http://127.0.0.1:43129".to_string(),
        )
        .await
        .unwrap();
        let assignment = AgentAssignment {
            id: "helper".to_string(),
            display_name: "Helper".to_string(),
            role: Some("Researcher".to_string()),
            persona: "Investigate carefully.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/main".to_string(),
            triage_model_id: "opencode/triage".to_string(),
            config_revision: 1,
            agenda_enabled: false,
        };

        let home = manager.materialize(&assignment, "token-one").await.unwrap();
        let persistent = openwork_root.join("agents/helper");
        let runtime = openwork_root.join("runtime").join(runtime_session_id);

        assert_eq!(home.work_root, persistent.join("work"));
        assert_eq!(
            home.state_file,
            persistent.join("engines/opencode/session.json")
        );
        assert_eq!(home.config_root, runtime.join("derived/helper/opencode"));
        assert_eq!(
            home.environment.get("XDG_CACHE_HOME").map(String::as_str),
            runtime.join("derived/helper/opencode/cache").to_str()
        );
        assert_eq!(
            home.environment.get("XDG_STATE_HOME").map(String::as_str),
            runtime.join("derived/helper/opencode/state").to_str()
        );
        assert_eq!(
            home.environment.get("HOME").map(String::as_str),
            persistent.to_str()
        );
        assert_eq!(
            home.environment
                .get("OPENWORK_RUNTIME_TOKEN_FILE")
                .map(String::as_str),
            runtime.join("agents/helper/runtime-token").to_str()
        );
        assert!(
            home.environment["PATH"].starts_with(&format!("{}:", runtime.join("bin").display()))
        );
        let managed_context = tokio::fs::read_to_string(persistent.join("AGENTS.md"))
            .await
            .unwrap();
        assert!(managed_context.contains("Investigate carefully."));
        assert_eq!(
            home.context_fingerprint,
            format!("sha256:{:x}", Sha256::digest(managed_context.as_bytes()))
        );
        assert_eq!(
            tokio::fs::read_to_string(runtime.join("agents/helper/runtime-token"))
                .await
                .unwrap(),
            "token-one"
        );
        assert_eq!(
            tokio::fs::read_link(runtime.join("bin/openwork"))
                .await
                .unwrap(),
            shim
        );
        for obsolete in ["bin", "memory", "notes", "skills", "workspace"] {
            assert!(!persistent.join(obsolete).exists(), "created {obsolete}");
        }

        tokio::fs::write(persistent.join("work/kept.txt"), b"keep")
            .await
            .unwrap();
        home.save_runtime_token("token-two").await.unwrap();
        assert_eq!(
            tokio::fs::read(persistent.join("work/kept.txt"))
                .await
                .unwrap(),
            b"keep"
        );
        assert_eq!(
            tokio::fs::read_to_string(runtime.join("agents/helper/runtime-token"))
                .await
                .unwrap(),
            "token-two"
        );
    }
}
