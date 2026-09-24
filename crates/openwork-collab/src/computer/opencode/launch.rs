//! 启动 OpenCode 所需的一切：在 PATH 上找到它、生成沙箱内的 argv、写派生配置、
//! 读出用户的登录信息（collaboration.md §3.1）。

use std::{collections::BTreeMap, path::PathBuf, process::Stdio, time::Duration};

use openwork_sandbox::{EngineConfinement, SandboxBackend, SandboxStatus, SandboxUnavailable};
use tokio::{io::AsyncReadExt, process::Command};

use super::{OpenCodeAdapter, TRIAGE_AGENT, atomic_write, secure_directory};
use crate::computer::engine::EngineError;

const INVENTORY_TIMEOUT: Duration = Duration::from_secs(3);

/// classify 专用的 `XDG_CONFIG_HOME` 子目录。OpenCode 会拼接各层配置里的
/// `instructions`，与主 Turn 共用配置目录就会把 Agent persona 带进分类调用。
const CLASSIFY_CONFIG_DIR: &str = "classify";
/// 用户 OpenCode 登录信息的上限（collaboration.md §3.1）：它经环境变量 `OPENCODE_AUTH_CONTENT` 进入沙箱，
/// 环境变量与 argv 共用 macOS 的 `ARG_MAX`（1 MiB）。
const MAX_AUTH_BYTES: u64 = 64 * 1024;

impl OpenCodeAdapter {
    /// 在 PATH 上查找 OpenCode；找不到时返回 `None`。
    pub(super) async fn locate(&self) -> Result<Option<PathBuf>, EngineError> {
        let mut command = Command::new("/usr/bin/which");
        command
            .arg(&self.executable)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(INVENTORY_TIMEOUT, command.output())
            .await
            .map_err(|_| EngineError::Timeout {
                operation: "executable lookup",
            })??;
        match output.status.code() {
            Some(0) => {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if path.is_empty() {
                    return Err(EngineError::Process {
                        detail: "/usr/bin/which printed no path".to_string(),
                    });
                }
                Ok(Some(PathBuf::from(path)))
            }
            Some(1) => Ok(None),
            _ => Err(EngineError::Process {
                detail: format!("/usr/bin/which failed with {}", output.status),
            }),
        }
    }

    /// 在 `confinement` 内运行 OpenCode 的 argv。OpenCode 本体可能装在 `$HOME` 下
    /// （官方安装脚本），所以放行它规范化后的路径；规范化会访问文件系统，放到阻塞线程里。
    pub(super) async fn confined_argv(
        &self,
        confinement: EngineConfinement,
        args: Vec<String>,
    ) -> Result<Vec<String>, EngineError> {
        if let SandboxStatus::Unavailable { reason } = self.sandbox.status() {
            return Err(SandboxUnavailable {
                reason: reason.clone(),
            }
            .into());
        }
        let executable = self.locate().await?.ok_or_else(|| EngineError::Missing {
            detail: format!("{} was not found on PATH", self.executable.display()),
        })?;
        let sandbox = self.sandbox.clone();
        tokio::task::spawn_blocking(move || {
            let executable = std::fs::canonicalize(&executable)?;
            let command = std::iter::once(executable.to_string_lossy().into_owned())
                .chain(args)
                .collect::<Vec<_>>();
            Ok(sandbox.confine(&confinement.with_readable_path(&executable), &command)?)
        })
        .await
        .map_err(|error| EngineError::Io(std::io::Error::other(error)))?
    }

    /// 用户自己的 OpenCode 登录信息；用户没有登录过时为 `None`。每次启动都重新读取，
    /// 用户重新登录后下一次 Turn 即生效。
    pub(super) async fn user_auth(&self) -> Result<Option<String>, EngineError> {
        let path = self.user_data_home.join("opencode").join("auth.json");
        let file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut content = String::new();
        file.take(MAX_AUTH_BYTES + 1)
            .read_to_string(&mut content)
            .await?;
        if content.len() as u64 > MAX_AUTH_BYTES {
            return Err(EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("OpenCode auth.json is larger than {MAX_AUTH_BYTES} bytes"),
            )));
        }
        Ok(Some(content))
    }
}

/// 正式 Turn 的环境：派生配置用 `instructions` 引用受管的 `AGENTS.md`。
pub(super) async fn turn_environment(
    config_root: &std::path::Path,
    instructions: &std::path::Path,
    environment: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, EngineError> {
    prepare_environment(config_root, Some(instructions), environment).await
}

/// 分类调用的环境：独立的配置目录，不加载 Agent persona。
pub(super) async fn classify_environment(
    config_root: &std::path::Path,
    environment: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, EngineError> {
    prepare_environment(&config_root.join(CLASSIFY_CONFIG_DIR), None, environment).await
}

/// 写入 OpenWork 派生的全局 OpenCode 配置，并返回让 OpenCode 只读这份配置的环境。
///
/// `OPENCODE_DISABLE_PROJECT_CONFIG` 同时关掉了 cwd 上方 AGENTS.md 的自动加载，
/// 所以 `instructions` 必须写成绝对路径（opencode `session/instruction.ts`）。
async fn prepare_environment(
    config_home: &std::path::Path,
    instructions: Option<&std::path::Path>,
    mut environment: BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, EngineError> {
    let mut config = serde_json::json!({"permission": {"*": "allow"}});
    if let Some(instructions) = instructions {
        let instructions = instructions.to_str().ok_or_else(|| {
            EngineError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Agent instructions path is not valid UTF-8",
            ))
        })?;
        config["instructions"] = serde_json::json!([instructions]);
    }
    let opencode_config = config_home.join("opencode");
    secure_directory(&opencode_config).await?;
    atomic_write(
        &opencode_config.join("opencode.json"),
        config.to_string().as_bytes(),
    )
    .await?;
    environment.insert(
        "XDG_CONFIG_HOME".to_string(),
        config_home.to_string_lossy().into_owned(),
    );
    environment.insert(
        "OPENCODE_DISABLE_PROJECT_CONFIG".to_string(),
        "1".to_string(),
    );
    Ok(environment)
}

/// 经 `OPENCODE_CONFIG_CONTENT` 注入的保留分类 agent：拒绝一切工具，只返回 JSON 判断。
pub(super) fn triage_config_content() -> String {
    serde_json::json!({
        "agent": {
            TRIAGE_AGENT: {
                "description": "OpenWork local classifier (tool-free)",
                "mode": "primary",
                "prompt": "Return only the requested JSON decision. Do not call tools.",
                "permission": {"*": "deny"}
            }
        }
    })
    .to_string()
}
