use std::path::PathBuf;

use openwork_models::model::ContentBlock;
use thiserror::Error;

use super::{
    ProjectInstructionError, ProjectInstructionLoader, ResolvedSystemContext, SystemContextPart,
    UserProjectContextError, UserProjectContextLoader,
};

/// Explicitly coordinates the context sources used to start one Turn.
///
/// This is intentionally concrete: new sources are added here when their
/// domain implementation exists, without introducing a registry ahead of need.
pub(crate) struct SystemContextBuilder {
    user_project_context: UserProjectContextLoader,
    project_instructions: ProjectInstructionLoader,
}

impl SystemContextBuilder {
    pub(crate) fn new(working_directory: impl Into<PathBuf>) -> Self {
        let working_directory = working_directory.into();
        Self {
            user_project_context: UserProjectContextLoader::new(working_directory.clone()),
            project_instructions: ProjectInstructionLoader::new(working_directory),
        }
    }

    pub(crate) async fn build(
        &self,
        agent_system_prompt: impl Into<String>,
    ) -> Result<ResolvedSystemContext, SystemContextBuildError> {
        let mut parts = vec![SystemContextPart::new(
            "core/agent-system",
            vec![ContentBlock::text(agent_system_prompt)],
        )];
        parts.push(self.user_project_context.load().await?);
        if let Some(project_instructions) = self.project_instructions.load().await? {
            parts.push(project_instructions);
        }
        Ok(ResolvedSystemContext::new(parts))
    }
}

#[derive(Debug, Error)]
pub(crate) enum SystemContextBuildError {
    #[error(transparent)]
    UserProjectContext(#[from] UserProjectContextError),
    #[error(transparent)]
    ProjectInstruction(#[from] ProjectInstructionError),
}

impl SystemContextBuildError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::UserProjectContext(_) => "user_project_context_error",
            Self::ProjectInstruction(_) => "project_instruction_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "openwork-context-builder-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).expect("workspace");
            Self { root }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[tokio::test]
    async fn coordinates_agent_and_project_context_sources() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.root.join("AGENTS.md"), "project rule\n").expect("instructions");

        let context = SystemContextBuilder::new(&workspace.root)
            .build("agent system")
            .await
            .expect("context");

        assert_eq!(context.parts().len(), 3);
        assert_eq!(context.parts()[0].key, "core/agent-system");
        assert_eq!(context.parts()[1].key, "runtime/user-project-context");
        assert_eq!(context.parts()[2].key, "project/AGENTS.md");
    }

    #[tokio::test]
    async fn missing_project_instructions_keeps_agent_context() {
        let workspace = TestWorkspace::new();

        let context = SystemContextBuilder::new(&workspace.root)
            .build("agent system")
            .await
            .expect("context");

        assert_eq!(context.parts().len(), 2);
        assert_eq!(context.parts()[0].key, "core/agent-system");
        assert_eq!(context.parts()[1].key, "runtime/user-project-context");
    }
}
