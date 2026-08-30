use std::{
    collections::BTreeMap,
    ffi::OsStr,
    future::Future,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};

use tokio::{fs, process::Command};

const SERVER_LABEL: &str = "io.openwork.collab.server";
const COMPUTER_LABEL: &str = "io.openwork.collab.computer";
const DEFAULT_RUNTIME_BIND: &str = "127.0.0.1:17843";
const UNLOAD_TIMEOUT: Duration = Duration::from_secs(15);
const UNLOAD_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchdRole {
    Server,
    Computer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchdStatus {
    pub server_loaded: bool,
    pub computer_loaded: bool,
}

#[derive(Clone, Debug)]
pub struct LaunchdEnvironment {
    pub database_url: String,
    pub redis_url: String,
    pub path: String,
    pub opencode_bin: Option<String>,
}

impl LaunchdEnvironment {
    pub fn from_process() -> Result<Self, LaunchdError> {
        Ok(Self {
            database_url: required_environment("DATABASE_URL")?,
            redis_url: required_environment("REDIS_URL")?,
            path: std::env::var("PATH").unwrap_or_else(|_| {
                "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_string()
            }),
            opencode_bin: std::env::var("OPENCODE_BIN").ok(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct LaunchdSupervisor {
    launch_agents: PathBuf,
    state_root: PathBuf,
    executable: PathBuf,
    server_arguments: Vec<String>,
    computer_arguments: Vec<String>,
    environment: LaunchdEnvironment,
    domain: String,
}

impl LaunchdSupervisor {
    pub fn new(
        user_home: PathBuf,
        state_root: PathBuf,
        executable: PathBuf,
        server_arguments: Vec<String>,
        computer_arguments: Vec<String>,
        environment: LaunchdEnvironment,
    ) -> Result<Self, LaunchdError> {
        if !executable.is_absolute() {
            return Err(LaunchdError::InvalidConfiguration(
                "launchd executable path must be absolute".to_string(),
            ));
        }
        if server_arguments.is_empty() || computer_arguments.is_empty() {
            return Err(LaunchdError::InvalidConfiguration(
                "both launchd roles require an argument".to_string(),
            ));
        }
        Ok(Self {
            launch_agents: user_home.join("Library/LaunchAgents"),
            state_root,
            executable,
            server_arguments,
            computer_arguments,
            environment,
            domain: format!("gui/{}", unsafe { libc::getuid() }),
        })
    }

    pub async fn ensure(&self, role: LaunchdRole) -> Result<bool, LaunchdError> {
        fs::create_dir_all(&self.launch_agents).await?;
        let logs = self.state_root.join("logs");
        fs::create_dir_all(&logs).await?;
        fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700)).await?;

        let plist_path = self.plist_path(role);
        let rendered = self.render(role)?;
        let changed = match fs::read_to_string(&plist_path).await {
            Ok(existing) => existing != rendered,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(error) => return Err(error.into()),
        };
        if changed {
            write_private_atomic(&plist_path, rendered.as_bytes()).await?;
        }

        let loaded = self.is_loaded(role).await;
        if changed && loaded {
            self.bootout_if_loaded(role).await?;
        }
        if changed || !loaded {
            self.bootstrap(role).await?;
        }
        Ok(changed)
    }

    pub async fn restart(&self, role: LaunchdRole) -> Result<(), LaunchdError> {
        let target = format!("{}/{}", self.domain, role.label());
        checked_launchctl([OsStr::new("kickstart"), OsStr::new("-k"), target.as_ref()]).await
    }

    pub async fn status(&self) -> LaunchdStatus {
        LaunchdStatus {
            server_loaded: self.is_loaded(LaunchdRole::Server).await,
            computer_loaded: self.is_loaded(LaunchdRole::Computer).await,
        }
    }

    pub async fn remove_all(&self) -> Result<(), LaunchdError> {
        for role in [LaunchdRole::Computer, LaunchdRole::Server] {
            self.bootout_if_loaded(role).await?;
            match fs::remove_file(self.plist_path(role)).await {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn plist_path(&self, role: LaunchdRole) -> PathBuf {
        self.launch_agents.join(format!("{}.plist", role.label()))
    }

    fn render(&self, role: LaunchdRole) -> Result<String, LaunchdError> {
        let executable = utf8_path(&self.executable)?;
        let log_path = self
            .state_root
            .join("logs")
            .join(format!("{}.log", role.file_stem()));
        let log_path = utf8_path(&log_path)?;
        let state_root = utf8_path(&self.state_root)?;
        let arguments = match role {
            LaunchdRole::Server => &self.server_arguments,
            LaunchdRole::Computer => &self.computer_arguments,
        };
        let mut environment = BTreeMap::from([("OPENWORK_COLLAB_HOME", state_root.to_string())]);
        match role {
            LaunchdRole::Server => {
                environment.insert("DATABASE_URL", self.environment.database_url.clone());
                environment.insert("REDIS_URL", self.environment.redis_url.clone());
                environment.insert(
                    "OPENWORK_COLLAB_RUNTIME_BIND",
                    DEFAULT_RUNTIME_BIND.to_string(),
                );
            }
            LaunchdRole::Computer => {
                environment.insert("PATH", self.environment.path.clone());
                environment.insert("OPENWORK_COLLAB_SUPERVISED", "1".to_string());
                if let Some(opencode_bin) = &self.environment.opencode_bin {
                    environment.insert("OPENCODE_BIN", opencode_bin.clone());
                }
            }
        }

        let mut program_arguments = format!("    <string>{}</string>\n", xml(executable));
        for argument in arguments {
            program_arguments.push_str(&format!("    <string>{}</string>\n", xml(argument)));
        }
        let mut environment_xml = String::new();
        for (key, value) in environment {
            environment_xml.push_str(&format!(
                "    <key>{}</key><string>{}</string>\n",
                xml(key),
                xml(&value)
            ));
        }

        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n\
  <key>Label</key><string>{label}</string>\n\
  <key>ProgramArguments</key>\n  <array>\n{program_arguments}  </array>\n\
  <key>RunAtLoad</key><true/>\n\
  <key>KeepAlive</key><true/>\n\
  <key>ProcessType</key><string>Background</string>\n\
  <key>ThrottleInterval</key><integer>5</integer>\n\
  <key>StandardOutPath</key><string>{log_path}</string>\n\
  <key>StandardErrorPath</key><string>{log_path}</string>\n\
  <key>EnvironmentVariables</key>\n  <dict>\n{environment_xml}  </dict>\n\
</dict>\n</plist>\n",
            label = role.label(),
            log_path = xml(log_path),
        ))
    }

    async fn is_loaded(&self, role: LaunchdRole) -> bool {
        let target = format!("{}/{}", self.domain, role.label());
        launchctl([OsStr::new("print"), target.as_ref()])
            .await
            .is_ok_and(|output| output.status.success())
    }

    async fn bootstrap(&self, role: LaunchdRole) -> Result<(), LaunchdError> {
        checked_launchctl([
            OsStr::new("bootstrap"),
            self.domain.as_ref(),
            self.plist_path(role).as_os_str(),
        ])
        .await
    }

    async fn bootout(&self, role: LaunchdRole) -> Result<(), LaunchdError> {
        let target = format!("{}/{}", self.domain, role.label());
        checked_launchctl([OsStr::new("bootout"), target.as_ref()]).await
    }

    async fn bootout_if_loaded(&self, role: LaunchdRole) -> Result<(), LaunchdError> {
        if !self.is_loaded(role).await {
            return Ok(());
        }
        match self.bootout(role).await {
            Ok(()) => self.wait_until_unloaded(role).await,
            Err(_) if !self.is_loaded(role).await => Ok(()),
            Err(error) => Err(error),
        }
    }

    async fn wait_until_unloaded(&self, role: LaunchdRole) -> Result<(), LaunchdError> {
        let supervisor = self.clone();
        let unloaded = wait_until_unloaded_with(UNLOAD_TIMEOUT, UNLOAD_POLL_INTERVAL, move || {
            let supervisor = supervisor.clone();
            async move { supervisor.is_loaded(role).await }
        })
        .await;
        if unloaded {
            Ok(())
        } else {
            Err(LaunchdError::UnloadTimeout(role.label()))
        }
    }
}

impl LaunchdRole {
    fn label(self) -> &'static str {
        match self {
            Self::Server => SERVER_LABEL,
            Self::Computer => COMPUTER_LABEL,
        }
    }

    fn file_stem(self) -> &'static str {
        match self {
            Self::Server => "collab-server",
            Self::Computer => "collab-computer",
        }
    }
}

async fn checked_launchctl<'a>(
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<(), LaunchdError> {
    let output = launchctl(arguments).await?;
    if output.status.success() {
        return Ok(());
    }
    Err(LaunchdError::Launchctl(
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    ))
}

async fn launchctl<'a>(
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> Result<Output, std::io::Error> {
    Command::new("/bin/launchctl")
        .args(arguments)
        .output()
        .await
}

async fn write_private_atomic(path: &Path, contents: &[u8]) -> Result<(), std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("launchd plist has no parent"))?;
    let temporary = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("openwork-launchd"),
        std::process::id()
    ));
    fs::write(&temporary, contents).await?;
    fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o600)).await?;
    if let Err(error) = fs::rename(&temporary, path).await {
        let _ = fs::remove_file(&temporary).await;
        return Err(error);
    }
    Ok(())
}

fn required_environment(name: &'static str) -> Result<String, LaunchdError> {
    std::env::var(name).map_err(|_| LaunchdError::MissingEnvironment(name))
}

fn utf8_path(path: &Path) -> Result<&str, LaunchdError> {
    path.to_str().ok_or_else(|| {
        LaunchdError::InvalidConfiguration(format!("path is not valid UTF-8: {}", path.display()))
    })
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

async fn wait_until_unloaded_with<F, Fut>(
    timeout: Duration,
    poll_interval: Duration,
    mut is_loaded: F,
) -> bool
where
    F: FnMut() -> Fut,
    Fut: Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if !is_loaded().await {
            return true;
        }
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return false;
        }
        tokio::time::sleep(poll_interval.min(deadline - now)).await;
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchdError {
    #[error("missing required environment variable {0}")]
    MissingEnvironment(&'static str),
    #[error("invalid launchd configuration: {0}")]
    InvalidConfiguration(String),
    #[error("launchctl failed: {0}")]
    Launchctl(String),
    #[error("launchd did not finish unloading {0} within 15 seconds")]
    UnloadTimeout(&'static str),
    #[error("launchd file operation failed: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        process::Stdio,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use super::{LaunchdEnvironment, LaunchdRole, LaunchdSupervisor, wait_until_unloaded_with};

    fn supervisor() -> LaunchdSupervisor {
        LaunchdSupervisor::new(
            "/Users/test".into(),
            "/Users/test/Library/Application Support/OpenWork & Local".into(),
            "/Applications/OpenWork <Preview>.app/Contents/MacOS/OpenWork".into(),
            vec!["--openwork-collab-server".to_string()],
            vec!["--openwork-collab-computer".to_string()],
            LaunchdEnvironment {
                database_url: "postgres://local".to_string(),
                redis_url: "redis://local".to_string(),
                path: "/opt/homebrew/bin:/usr/bin".to_string(),
                opencode_bin: Some("/opt/homebrew/bin/opencode".to_string()),
            },
        )
        .unwrap()
    }

    #[test]
    fn server_plist_has_only_server_dependencies_and_escapes_xml() {
        let plist = supervisor().render(LaunchdRole::Server).unwrap();
        assert!(plist.contains("io.openwork.collab.server"));
        assert!(plist.contains("--openwork-collab-server"));
        assert!(plist.contains("OpenWork &lt;Preview&gt;.app"));
        assert!(plist.contains("OpenWork &amp; Local"));
        assert!(plist.contains("DATABASE_URL"));
        assert!(plist.contains("OPENWORK_COLLAB_RUNTIME_BIND"));
        assert!(!plist.contains("<key>PATH</key>"));
        assert!(!plist.contains("OPENCODE_BIN"));
        assert!(!plist.contains("OPENWORK_COLLAB_SUPERVISED"));
    }

    #[test]
    fn computer_plist_is_supervised_without_database_or_redis_access() {
        let plist = supervisor().render(LaunchdRole::Computer).unwrap();
        assert!(plist.contains("io.openwork.collab.computer"));
        assert!(plist.contains("--openwork-collab-computer"));
        assert!(plist.contains("OPENWORK_COLLAB_SUPERVISED"));
        assert!(plist.contains("OPENCODE_BIN"));
        assert!(!plist.contains("DATABASE_URL"));
        assert!(!plist.contains("REDIS_URL"));
    }

    #[test]
    fn rendered_plists_are_valid_apple_property_lists() {
        for role in [LaunchdRole::Server, LaunchdRole::Computer] {
            let plist = supervisor().render(role).unwrap();
            let mut child = std::process::Command::new("/usr/bin/plutil")
                .args(["-lint", "-"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(plist.as_bytes())
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "plutil rejected {role:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[tokio::test]
    async fn unload_waits_until_launchd_finishes_async_cleanup() {
        let probes = Arc::new(AtomicUsize::new(0));
        let observed = probes.clone();

        let unloaded =
            wait_until_unloaded_with(Duration::from_secs(1), Duration::ZERO, move || {
                let observed = observed.clone();
                async move { observed.fetch_add(1, Ordering::SeqCst) < 2 }
            })
            .await;

        assert!(unloaded);
        assert_eq!(probes.load(Ordering::SeqCst), 3);
    }
}
