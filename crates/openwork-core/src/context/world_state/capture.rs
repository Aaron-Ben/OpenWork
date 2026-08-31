//! 从权威来源读取三个 section 的当前值。
//!
//! 这是 world state 唯一的 IO 入口。`world_state/` 其余部分是纯逻辑，只处理
//! 比较与渲染。
//!
//! # 与 `SystemContextBuilder` 的关系
//!
//! 二 C-1 阶段两者并存：前缀仍由 builder 组装，capture 还没人调用。二 C-2 把
//! 三个 part 从 builder 删掉、改由这里供给 Conversation。
//!
//! **两条路径必须共用同一份正文构造**（各 loader 的 `load_body`），否则搬家那
//! 一步会连带改变模型看到的内容，出了问题分不清是位置错了还是内容错了。

use std::collections::BTreeSet;
use std::path::PathBuf;

use thiserror::Error;

use crate::skills::SkillRoots;

use super::super::project_instructions::{ProjectInstructionError, ProjectInstructionLoader};
use super::super::skill_catalog::SkillCatalogLoader;
use super::super::user_project::{UserProjectContextError, UserProjectContextLoader};
use super::{AgentsMdState, ProjectContextState, SkillsCatalogState, WorldState};

pub(crate) struct WorldStateCapture {
    user_project: UserProjectContextLoader,
    project_instructions: ProjectInstructionLoader,
    skill_catalog: SkillCatalogLoader,
}

impl WorldStateCapture {
    pub(crate) fn new(working_directory: impl Into<PathBuf>, skill_roots: SkillRoots) -> Self {
        let working_directory = working_directory.into();
        Self {
            user_project: UserProjectContextLoader::new(working_directory.clone()),
            project_instructions: ProjectInstructionLoader::new(working_directory),
            skill_catalog: SkillCatalogLoader::new(skill_roots),
        }
    }

    pub(crate) fn with_disabled_skills(mut self, disabled_names: BTreeSet<String>) -> Self {
        self.skill_catalog = self.skill_catalog.with_disabled_names(disabled_names);
        self
    }

    /// 读一次三个来源，组成本次采样的 `WorldState`。
    ///
    /// Skill 扫描是阻塞 IO，与 `SystemContextBuilder::build` 一样放到
    /// `spawn_blocking`；skill 告警照旧记进日志，不进模型上下文。
    pub(crate) async fn capture(&self) -> Result<WorldState, WorldStateCaptureError> {
        let project_context = self.user_project.load_body().await?;
        let agents_md = self.project_instructions.load_body().await?;
        let skill_catalog = self.skill_catalog.clone();
        let (skills_catalog, warnings) =
            tokio::task::spawn_blocking(move || skill_catalog.load_body())
                .await
                .map_err(WorldStateCaptureError::SkillCatalogTask)?;
        for warning in warnings {
            tracing::warn!(
                path = %warning.path,
                reason = %warning.reason,
                "skill catalog warning"
            );
        }

        Ok(WorldState {
            project_context: ProjectContextState::new(project_context),
            agents_md: AgentsMdState::new(agents_md),
            skills_catalog: SkillsCatalogState::new(skills_catalog),
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum WorldStateCaptureError {
    #[error(transparent)]
    UserProjectContext(#[from] UserProjectContextError),
    #[error(transparent)]
    ProjectInstruction(#[from] ProjectInstructionError),
    #[error("skill catalog filesystem task failed")]
    SkillCatalogTask(#[source] tokio::task::JoinError),
}

impl WorldStateCaptureError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::UserProjectContext(_) => "user_project_context_error",
            Self::ProjectInstruction(_) => "project_instruction_error",
            Self::SkillCatalogTask(_) => "skill_catalog_task_error",
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
        agents: PathBuf,
    }

    impl TestWorkspace {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "openwork-world-state-capture-{}",
                Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&root).expect("workspace");
            let agents = root.join("user-home/.agents/skills");
            fs::create_dir_all(&agents).expect("agents skill root");
            Self { root, agents }
        }

        fn write_skill(&self, name: &str, source: &str) {
            let directory = self.agents.join(name);
            fs::create_dir_all(&directory).expect("skill directory");
            fs::write(directory.join("SKILL.md"), source).expect("skill");
        }

        fn capture(&self) -> WorldStateCapture {
            WorldStateCapture::new(
                &self.root,
                SkillRoots {
                    agents: Some(self.agents.clone()),
                },
            )
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    /// 三段正文各自的形态。
    ///
    /// 原先这里比对的是 `SystemContextBuilder` 的输出，用来锁住"二 C-2 的搬家只
    /// 改变位置和 role、不改变内容"。搬完之后前缀里已经没有这三段可比，那条过渡
    /// 守卫的使命结束；改为直接锁住各自的形态。端到端的内容验证在
    /// `tests/session_runtime.rs` 的接线测试里。
    ///
    /// `agents_md` 给的是**裸文件内容**：`<project_instructions>` 标记由
    /// `agents_md.rs` 在渲染时加，不在捕获这一层。两层混了的话，比较用带标记的、
    /// 基线存裸的，每次都会判成"变了"。
    #[tokio::test]
    async fn each_captured_body_has_its_own_shape() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.root.join("AGENTS.md"), "永远先跑测试\n").expect("instructions");
        workspace.write_skill(
            "commit",
            "---\nname: commit\ndescription: Create a commit.\n---\nBody\n",
        );

        let world = workspace.capture().capture().await.expect("capture");

        assert!(
            world
                .project_context
                .body_for_test()
                .starts_with("<user_project_context"),
            "项目上下文自带标记"
        );
        assert_eq!(
            world.agents_md.body_for_test().as_deref(),
            Some("永远先跑测试\n"),
            "AGENTS.md 在这一层是裸文件内容，不带标记"
        );
        assert!(
            world
                .skills_catalog
                .body_for_test()
                .expect("catalog")
                .starts_with("<available_skills>"),
            "skill 清单自带标记"
        );
    }

    /// 缺失的来源要如实变成 `None`，不能渲染成空正文。
    ///
    /// 空正文和缺失在 §8.2 里是两种结果：前者要发一条空消息，后者应当什么都不发。
    #[tokio::test]
    async fn missing_sources_capture_as_absent_rather_than_empty() {
        let workspace = TestWorkspace::new();

        let world = workspace.capture().capture().await.expect("capture");

        assert_eq!(world.agents_md.body_for_test(), None);
        assert_eq!(world.skills_catalog.body_for_test(), None);
        assert!(!world.project_context.body_for_test().is_empty());
    }

    /// 输入不变时两次 capture 逐字节相同。
    ///
    /// 这是"没变就不发"的前提：capture 只要有一点不确定性（目录序、路径形态），
    /// 每次采样都会判成变化，于是每个 Model Call 都往历史里加三条消息。
    #[tokio::test]
    async fn two_captures_over_unchanged_sources_are_equal() {
        let workspace = TestWorkspace::new();
        fs::write(workspace.root.join("AGENTS.md"), "规范\n").expect("instructions");
        workspace.write_skill(
            "zulu",
            "---\nname: zulu\ndescription: Use zulu.\n---\nBody\n",
        );
        workspace.write_skill(
            "alpha",
            "---\nname: alpha\ndescription: Use alpha.\n---\nBody\n",
        );

        let first = workspace.capture().capture().await.expect("first");
        let second = workspace.capture().capture().await.expect("second");

        assert_eq!(first, second);
    }

    /// 关掉的 skill 不出现在 capture 里。
    #[tokio::test]
    async fn disabled_skills_do_not_reach_the_captured_catalog() {
        let workspace = TestWorkspace::new();
        workspace.write_skill(
            "commit",
            "---\nname: commit\ndescription: Create a commit.\n---\nBody\n",
        );

        let world = workspace
            .capture()
            .with_disabled_skills(BTreeSet::from(["commit".to_string()]))
            .capture()
            .await
            .expect("capture");

        assert_eq!(world.skills_catalog.body_for_test(), None);
    }
}
