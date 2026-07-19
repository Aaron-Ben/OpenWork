mod edit;
mod glob;
mod grep;
mod list;
mod read;
mod write;

pub(crate) use edit::EditTool;
pub(crate) use glob::GlobTool;
pub(crate) use grep::GrepTool;
pub(crate) use list::ListTool;
pub(crate) use read::ReadTool;
pub(crate) use write::WriteTool;

#[cfg(test)]
pub(super) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        pub fn new(label: &str) -> Self {
            let id = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "openwork-tools-{label}-{}-{id}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("create test directory");
            Self { path }
        }

        pub fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
