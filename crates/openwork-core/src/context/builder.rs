use std::collections::BTreeSet;
use std::path::PathBuf;

use openwork_models::model::ContentBlock;
use thiserror::Error;

use crate::skills::SkillRoots;

use super::world_state::WorldStateCaptureError;
use super::{ResolvedSystemContext, SystemContextPart};

/// Explicitly coordinates the context sources used to start one Turn.
///
/// This is intentionally concrete: new sources are added here when their
/// domain implementation exists, without introducing a registry ahead of need.
pub(crate) struct SystemContextBuilder;

impl SystemContextBuilder {
    pub(crate) fn new(working_directory: impl Into<PathBuf>, skill_roots: SkillRoots) -> Self {
        let _ = (working_directory.into(), skill_roots);
        Self
    }

    pub(crate) fn with_disabled_skills(self, disabled_names: BTreeSet<String>) -> Self {
        let _ = disabled_names;
        self
    }

    pub(crate) async fn build(
        &self,
        agent_system_prompt: impl Into<String>,
    ) -> Result<ResolvedSystemContext, SystemContextBuildError> {
        Ok(ResolvedSystemContext::new(vec![SystemContextPart::new(
            "core/agent-system",
            vec![ContentBlock::text(agent_system_prompt)],
        )]))
    }
}

#[derive(Debug, Error)]
pub(crate) enum SystemContextBuildError {
    #[error(transparent)]
    WorldState(#[from] WorldStateCaptureError),
}

impl SystemContextBuildError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::WorldState(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use uuid::Uuid;

    use super::*;
    use crate::skills::SkillRoots;

    struct TestWorkspace {
        root: PathBuf,
        agents: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "openwork-context-builder-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).expect("workspace");
            let agents = root.join("user-home/.agents/skills");
            fs::create_dir_all(&agents).expect("agents skill root");
            Self { root, agents }
        }

        fn write_agents_skill(&self, name: &str, source: &str) {
            let directory = self.agents.join(name);
            fs::create_dir_all(&directory).expect("skill directory");
            fs::write(directory.join("SKILL.md"), source).expect("skill");
        }

        fn roots(&self) -> SkillRoots {
            SkillRoots {
                agents: Some(self.agents.clone()),
            }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// System 前缀只剩 Agent 系统提示一段（§8.0）。
    ///
    /// 项目上下文、AGENTS.md、skill 清单都已下沉为 world-state section，靠追加
    /// 消息进入 Conversation。它们只要还在前缀里，任何一个变化都会作废整段对话
    /// 的缓存——这正是本次改造要消除的东西。
    #[tokio::test]
    async fn the_system_prefix_is_only_the_agent_system_prompt() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.root.join("AGENTS.md"), "project rule\n").expect("instructions");
        workspace.write_agents_skill(
            "commit",
            "---\nname: commit\ndescription: Create a commit.\n---\nBody\n",
        );

        let context = SystemContextBuilder::new(&workspace.root, workspace.roots())
            .build("agent system")
            .await
            .expect("context");

        assert_eq!(context.parts().len(), 1);
        assert_eq!(context.parts()[0].key, "core/agent-system");
        assert_eq!(
            context.parts()[0].content,
            [ContentBlock::text("agent system")]
        );
    }

    /// 前缀在整个 Session 内逐字节不变（§3.1 不变量 6）。
    ///
    /// 这是本次改造的收益本身：工作区内容怎么变都不该影响前缀字节，否则
    /// prompt cache 每轮作废，长对话每轮按全价重新 prefill。
    #[tokio::test]
    async fn workspace_changes_do_not_alter_the_prefix() {
        let workspace = TestWorkspace::new();

        let before = SystemContextBuilder::new(&workspace.root, workspace.roots())
            .build("agent system")
            .await
            .expect("first");

        fs::write(workspace.root.join("AGENTS.md"), "新增的项目规范\n").expect("instructions");
        fs::create_dir_all(workspace.root.join("新增顶层目录")).expect("new top level entry");
        workspace.write_agents_skill(
            "review",
            "---\nname: review\ndescription: Review changes.\n---\nBody\n",
        );

        let after = SystemContextBuilder::new(&workspace.root, workspace.roots())
            .build("agent system")
            .await
            .expect("second");

        assert_eq!(
            serde_json::to_vec(before.parts()).expect("before bytes"),
            serde_json::to_vec(after.parts()).expect("after bytes"),
        );
    }

    /// 停用 skill 也不该动到前缀。
    #[tokio::test]
    async fn disabling_a_skill_does_not_alter_the_prefix() {
        let workspace = TestWorkspace::new();
        workspace.write_agents_skill(
            "commit",
            "---\nname: commit\ndescription: Create a commit.\n---\nBody\n",
        );

        let enabled = SystemContextBuilder::new(&workspace.root, workspace.roots())
            .build("agent system")
            .await
            .expect("enabled");
        let disabled = SystemContextBuilder::new(&workspace.root, workspace.roots())
            .with_disabled_skills(BTreeSet::from(["commit".to_string()]))
            .build("agent system")
            .await
            .expect("disabled");

        assert_eq!(enabled.parts(), disabled.parts());
    }
}
