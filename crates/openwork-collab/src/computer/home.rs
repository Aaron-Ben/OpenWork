use std::{collections::BTreeMap, path::PathBuf};

use openwork_sandbox::{EngineConfinement, SandboxEnvironment};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::protocol::AgentAssignment;

pub struct HomeManager {
    openwork_root: PathBuf,
    runtime_root: PathBuf,
    runtime_bin: PathBuf,
    runtime_base_url: String,
    /// 已规范化的 shim 可执行文件；Engine 沙箱要放行它的读取。
    shim_executable: PathBuf,
    sandbox_environment: SandboxEnvironment,
}

pub struct AgentHome {
    pub work_root: PathBuf,
    pub config_root: PathBuf,
    /// 受管的 `AGENTS.md`；Engine adapter 负责让 Engine 把它作为系统指令加载。
    pub instructions_file: PathBuf,
    /// 本 Agent 的 Engine 进程树只能在这个围栏内读写（collaboration.md §3.1）。
    pub confinement: EngineConfinement,
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
        sandbox_environment: SandboxEnvironment,
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
        let shim_executable = tokio::fs::canonicalize(&shim_executable).await?;
        Ok(Self {
            openwork_root,
            runtime_root,
            runtime_bin,
            runtime_base_url,
            shim_executable,
            sandbox_environment,
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
        let instructions_file = root.join("AGENTS.md");
        atomic_write(&instructions_file, managed_context.as_bytes(), 0o600).await?;
        atomic_write(&token_file, runtime_token.as_bytes(), 0o600).await?;

        let data_home = engine_root.join("data");
        secure_directory(&data_home).await?;
        let confinement = self
            .confinement(
                root.clone(),
                derived_root(&config_root).to_path_buf(),
                token_file.clone(),
            )
            .await?;

        let original_path = std::env::var("PATH").unwrap_or_default();
        let environment = BTreeMap::from([
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
            // 每个 Agent 独立的 Engine 数据目录：沙箱不放行用户自己的 Engine 数据（§3.1）。
            (
                "XDG_DATA_HOME".to_string(),
                data_home.to_string_lossy().into_owned(),
            ),
        ]);
        Ok(AgentHome {
            work_root,
            config_root,
            instructions_file,
            confinement,
            state_file: engine_root.join("session.json"),
            context_fingerprint,
            environment,
            token_file,
        })
    }
}

impl HomeManager {
    /// 本 Agent 的围栏：可写 Agent 根与派生配置根；`$HOME` 内另外只放行本 Agent 的 token、
    /// runtime `bin/` 与 shim 本体。规范化路径会访问文件系统，所以放到阻塞线程里。
    async fn confinement(
        &self,
        agent_root: PathBuf,
        derived_root: PathBuf,
        token_file: PathBuf,
    ) -> Result<EngineConfinement, HomeError> {
        let environment = self.sandbox_environment.clone();
        let runtime_bin = self.runtime_bin.clone();
        let shim_executable = self.shim_executable.clone();
        Ok(tokio::task::spawn_blocking(move || {
            EngineConfinement::new(&environment)
                .with_writable_root(&agent_root)
                .with_writable_root(&derived_root)
                .with_readable_path(&token_file)
                .with_readable_path(&runtime_bin)
                .with_readable_path(&shim_executable)
        })
        .await?)
    }
}

/// `derived/<agent-id>/<engine-id>` 的上一级：同一 Agent 所有 Engine 的派生配置。
fn derived_root(config_root: &std::path::Path) -> &std::path::Path {
    config_root
        .parent()
        .expect("config root is derived/<agent-id>/<engine-id>")
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
    #[error("Agent sandbox setup stopped unexpectedly: {0}")]
    Blocking(#[from] tokio::task::JoinError),
    #[error("Agent home I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(all(test, unix))]
mod tests {
    use sha2::{Digest, Sha256};

    use openwork_sandbox::SandboxEnvironment;

    use crate::protocol::AgentAssignment;

    use super::HomeManager;

    fn assignment(id: &str) -> AgentAssignment {
        AgentAssignment {
            id: id.to_string(),
            display_name: id.to_string(),
            role: None,
            persona: "Work carefully.".to_string(),
            engine_id: "opencode".to_string(),
            main_model_id: "opencode/main".to_string(),
            triage_model_id: "opencode/triage".to_string(),
            config_revision: 1,
            agenda_enabled: false,
        }
    }

    /// collaboration.md §12 #10：Engine 进程只能写本 Agent 的目录；`$HOME` 内读不到其他 Agent
    /// 的目录与 token、用户自己的 OpenCode 数据和其他文件，但读得到本 Agent 的 token。
    /// `$HOME` 是临时目录里的假主目录，临时根不给写权限，因此只有 Agent 自己的目录可写。
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn acc_10_an_agent_engine_cannot_reach_another_agent_or_the_user_home() {
        use tokio_util::sync::CancellationToken;

        use crate::computer::engine::{EngineAdapter, EngineRuntimeConfig, TurnRequest};
        use crate::computer::opencode::OpenCodeAdapter;

        let directory = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(directory.path()).unwrap();
        let home = root.join("home");
        let shim = root.join("shim");
        tokio::fs::create_dir_all(home.join("Documents"))
            .await
            .unwrap();
        tokio::fs::create_dir_all(home.join(".local/share/opencode"))
            .await
            .unwrap();
        tokio::fs::write(&shim, b"binary").await.unwrap();
        tokio::fs::write(home.join("Documents/plan.md"), b"user plan")
            .await
            .unwrap();
        tokio::fs::write(home.join(".local/share/opencode/auth.json"), b"{}")
            .await
            .unwrap();
        let manager = HomeManager::prepare(
            home.join(".openwork"),
            &format!("runtime-{}", "b".repeat(32)),
            shim,
            "http://127.0.0.1:43129".to_string(),
            SandboxEnvironment::new(home.clone(), Vec::new(), Vec::new()),
        )
        .await
        .unwrap();
        let alpha = manager
            .materialize(&assignment("alpha"), "alpha-token")
            .await
            .unwrap();
        let beta = manager
            .materialize(&assignment("beta"), "beta-token")
            .await
            .unwrap();
        let checks = [
            (
                "own_token",
                "read",
                alpha.environment["OPENWORK_RUNTIME_TOKEN_FILE"].clone(),
            ),
            (
                "beta_token",
                "read",
                beta.environment["OPENWORK_RUNTIME_TOKEN_FILE"].clone(),
            ),
            (
                "beta_persona",
                "read",
                home.join(".openwork/agents/beta/AGENTS.md")
                    .display()
                    .to_string(),
            ),
            (
                "user_auth",
                "read",
                home.join(".local/share/opencode/auth.json")
                    .display()
                    .to_string(),
            ),
            (
                "documents",
                "read",
                home.join("Documents/plan.md").display().to_string(),
            ),
            (
                "own_work",
                "write",
                alpha.work_root.join("note.txt").display().to_string(),
            ),
            (
                "beta_work",
                "write",
                beta.work_root.join("planted.txt").display().to_string(),
            ),
            (
                "home",
                "write",
                home.join("planted.txt").display().to_string(),
            ),
        ];
        let probes = checks
            .iter()
            .map(|(name, kind, path)| match *kind {
                "read" => format!("if cat '{path}' >/dev/null 2>&1; then r=\"$r {name}=read\"; else r=\"$r {name}=denied\"; fi"),
                _ => format!("if printf x > '{path}' 2>/dev/null; then r=\"$r {name}=wrote\"; else r=\"$r {name}=denied\"; fi"),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let executable = root.join("opencode-probe");
        tokio::fs::write(
            &executable,
            format!(
                "#!/bin/sh\ncat >/dev/null\nr=''\n{probes}\nprintf '%s\\n' '{{\"type\":\"step_start\",\"sessionID\":\"ses_probe\"}}' \"{{\\\"type\\\":\\\"text\\\",\\\"part\\\":{{\\\"text\\\":\\\"$r\\\"}}}}\"\n"
            ),
        )
        .await
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
                .await
                .unwrap();
        }
        let adapter = OpenCodeAdapter::new(
            executable,
            openwork_sandbox::Seatbelt::probe(openwork_sandbox::SANDBOX_EXEC),
            home.join(".local/share"),
        );
        let mut runtime = adapter
            .create_agent_runtime(EngineRuntimeConfig {
                home: alpha.work_root.clone(),
                config_root: alpha.config_root.clone(),
                instructions_file: alpha.instructions_file.clone(),
                confinement: alpha.confinement.clone(),
                state_file: alpha.state_file.clone(),
                context_fingerprint: alpha.context_fingerprint.clone(),
                model: "opencode/main".to_string(),
                environment: alpha.environment.clone(),
                turn_timeout: None,
            })
            .await
            .unwrap();

        let result = runtime
            .run_turn(TurnRequest {
                prompt: "probe".to_string(),
                cancellation: CancellationToken::new(),
            })
            .await
            .unwrap();

        assert_eq!(
            result.text,
            " own_token=read beta_token=denied beta_persona=denied user_auth=denied \
             documents=denied own_work=wrote beta_work=denied home=denied"
        );
        assert!(!beta.work_root.join("planted.txt").exists());
        assert!(!home.join("planted.txt").exists());
    }

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
            SandboxEnvironment::new(directory.path().to_path_buf(), Vec::new(), Vec::new()),
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
            home.environment.get("XDG_DATA_HOME").map(String::as_str),
            persistent.join("engines/opencode/data").to_str()
        );
        assert!(persistent.join("engines/opencode/data").is_dir());
        assert_eq!(
            home.environment
                .get("OPENWORK_RUNTIME_TOKEN_FILE")
                .map(String::as_str),
            runtime.join("agents/helper/runtime-token").to_str()
        );
        assert!(
            home.environment["PATH"].starts_with(&format!("{}:", runtime.join("bin").display()))
        );
        assert_eq!(home.instructions_file, persistent.join("AGENTS.md"));
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
