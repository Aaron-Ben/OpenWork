use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use openwork_models::model::{ContentBlock, Message};
use openwork_tools::FileChangeArtifact;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use super::reminder::{ReminderSection, render_system_reminder};

const MAX_EDITED_PATHS: usize = 128;
const MAX_EDITED_PATH_CHARS: usize = 1_024;
pub(super) const MAX_REMINDER_CHARS: usize = 32 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionStateEntry {
    pub schema_version: u16,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionStateWarning {
    pub contributor_key: String,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionRuntimeState {
    pub schema_version: u16,
    pub edited_paths: Vec<String>,
    pub extensions: BTreeMap<String, CompactionStateEntry>,
    pub warnings: Vec<CompactionStateWarning>,
}

impl Default for CompactionRuntimeState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            edited_paths: Vec::new(),
            extensions: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactionStateFailurePolicy {
    RequiredWhenEnabled,
    BestEffort,
}

pub struct CompactionStateCollectInput<'a> {
    pub messages: &'a [Message],
}

#[derive(Debug, Error)]
pub enum CompactionStateError {
    #[error("compaction state contributor key must not be blank")]
    BlankKey,
    #[error("duplicate compaction state contributor key: {0}")]
    DuplicateKey(String),
    #[error("unsupported compaction runtime state schema version: {0}")]
    UnsupportedRuntimeStateVersion(u16),
    #[error("compaction state contributor {key} failed: {message}")]
    Contributor { key: String, message: String },
    #[error("compaction runtime reminder exceeded {MAX_REMINDER_CHARS} characters")]
    ReminderTooLarge,
    #[error("compaction runtime reminder was invalid: {0}")]
    InvalidReminder(String),
}

#[async_trait]
pub trait CompactionStateContributor: Send + Sync {
    fn key(&self) -> &'static str;
    fn schema_version(&self) -> u16;
    fn failure_policy(&self) -> CompactionStateFailurePolicy;

    async fn collect(
        &self,
        input: &CompactionStateCollectInput<'_>,
    ) -> Result<Option<Value>, CompactionStateError>;

    fn render(
        &self,
        schema_version: u16,
        value: &Value,
    ) -> Result<Option<ReminderSection>, CompactionStateError>;
}

pub struct CompactionStateCollector {
    contributors: Vec<Box<dyn CompactionStateContributor>>,
}

impl Default for CompactionStateCollector {
    fn default() -> Self {
        Self::new(vec![Box::new(FileChangeStateContributor)])
            .expect("built-in compaction state contributor keys are valid")
    }
}

impl CompactionStateCollector {
    pub fn new(
        mut contributors: Vec<Box<dyn CompactionStateContributor>>,
    ) -> Result<Self, CompactionStateError> {
        contributors.sort_by_key(|contributor| contributor.key());
        let mut keys = BTreeSet::new();
        for contributor in &contributors {
            let key = contributor.key();
            if key.trim().is_empty() {
                return Err(CompactionStateError::BlankKey);
            }
            if !keys.insert(key) {
                return Err(CompactionStateError::DuplicateKey(key.to_string()));
            }
        }
        Ok(Self { contributors })
    }

    pub fn with_contributor(
        mut self,
        contributor: Box<dyn CompactionStateContributor>,
    ) -> Result<Self, CompactionStateError> {
        self.contributors.push(contributor);
        Self::new(self.contributors)
    }

    #[cfg(test)]
    pub async fn collect(
        &self,
        messages: &[Message],
    ) -> Result<(CompactionRuntimeState, String), CompactionStateError> {
        self.collect_with_base(messages, CompactionRuntimeState::default())
            .await
    }

    pub async fn collect_with_base(
        &self,
        messages: &[Message],
        mut state: CompactionRuntimeState,
    ) -> Result<(CompactionRuntimeState, String), CompactionStateError> {
        if state.schema_version != 1 {
            return Err(CompactionStateError::UnsupportedRuntimeStateVersion(
                state.schema_version,
            ));
        }
        let input = CompactionStateCollectInput { messages };
        let mut sections = Vec::new();
        state.warnings.clear();

        for contributor in &self.contributors {
            let key = contributor.key();
            let value = match contributor.collect(&input).await {
                Ok(value) => value,
                Err(error)
                    if contributor.failure_policy() == CompactionStateFailurePolicy::BestEffort =>
                {
                    state.warnings.push(CompactionStateWarning {
                        contributor_key: key.to_string(),
                        code: "collection_failed".to_string(),
                    });
                    let _ = error;
                    None
                }
                Err(error) => return Err(error),
            };
            if key == FileChangeStateContributor.key() {
                if let Some(value) = value {
                    state.edited_paths = edited_paths_from_value(&value).map_err(|message| {
                        CompactionStateError::Contributor {
                            key: key.to_string(),
                            message,
                        }
                    })?;
                }
                let rendered = json!({ "editedPaths": &state.edited_paths });
                if let Some(section) =
                    contributor.render(contributor.schema_version(), &rendered)?
                {
                    sections.push(section);
                }
            } else {
                if let Some(value) = value {
                    state.extensions.insert(
                        key.to_string(),
                        CompactionStateEntry {
                            schema_version: contributor.schema_version(),
                            value,
                        },
                    );
                }
                if let Some(entry) = state.extensions.get(key)
                    && let Some(section) = contributor.render(entry.schema_version, &entry.value)?
                {
                    sections.push(section);
                }
            }
        }

        Ok((state, render_system_reminder(&sections)?))
    }
}

struct FileChangeStateContributor;

#[async_trait]
impl CompactionStateContributor for FileChangeStateContributor {
    fn key(&self) -> &'static str {
        "file_changes"
    }

    fn schema_version(&self) -> u16 {
        1
    }

    fn failure_policy(&self) -> CompactionStateFailurePolicy {
        CompactionStateFailurePolicy::RequiredWhenEnabled
    }

    async fn collect(
        &self,
        input: &CompactionStateCollectInput<'_>,
    ) -> Result<Option<Value>, CompactionStateError> {
        let mut paths = BTreeSet::new();
        for message in input.messages {
            for block in &message.content {
                let ContentBlock::ToolResult(result) = block else {
                    continue;
                };
                for artifact in &result.artifacts {
                    if artifact.kind != "file_change" {
                        continue;
                    }
                    let change =
                        FileChangeArtifact::from_result_artifact(artifact).map_err(|error| {
                            CompactionStateError::Contributor {
                                key: self.key().to_string(),
                                message: format!("invalid file_change artifact: {error}"),
                            }
                        })?;
                    let path: String = change.path.chars().take(MAX_EDITED_PATH_CHARS).collect();
                    if path.trim().is_empty() {
                        continue;
                    }
                    if !change.undone {
                        paths.insert(path);
                    }
                }
            }
        }
        let paths: Vec<_> = paths.into_iter().take(MAX_EDITED_PATHS).collect();
        Ok(Some(json!({ "editedPaths": paths })))
    }

    fn render(
        &self,
        schema_version: u16,
        value: &Value,
    ) -> Result<Option<ReminderSection>, CompactionStateError> {
        if schema_version != self.schema_version() {
            return Err(CompactionStateError::Contributor {
                key: self.key().to_string(),
                message: format!("unsupported schema version: {schema_version}"),
            });
        }
        let paths = edited_paths_from_value(value).map_err(|message| {
            CompactionStateError::Contributor {
                key: self.key().to_string(),
                message,
            }
        })?;
        if paths.is_empty() {
            return Ok(None);
        }
        Ok(Some(ReminderSection {
            title: "Edited paths".to_string(),
            lines: paths.into_iter().map(|path| format!("- {path}")).collect(),
        }))
    }
}

fn edited_paths_from_value(value: &Value) -> Result<Vec<String>, String> {
    value
        .get("editedPaths")
        .and_then(Value::as_array)
        .ok_or_else(|| "editedPaths must be an array".to_string())?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "editedPaths entries must be strings".to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use openwork_models::model::{
        Message, Role, ToolResultArtifact, ToolResultBlock, ToolResultState,
    };
    use serde_json::json;

    use super::*;

    struct FailingBestEffortContributor;

    #[async_trait]
    impl CompactionStateContributor for FailingBestEffortContributor {
        fn key(&self) -> &'static str {
            "future_best_effort"
        }

        fn schema_version(&self) -> u16 {
            1
        }

        fn failure_policy(&self) -> CompactionStateFailurePolicy {
            CompactionStateFailurePolicy::BestEffort
        }

        async fn collect(
            &self,
            _input: &CompactionStateCollectInput<'_>,
        ) -> Result<Option<Value>, CompactionStateError> {
            Err(CompactionStateError::Contributor {
                key: self.key().to_string(),
                message: "unavailable".to_string(),
            })
        }

        fn render(
            &self,
            _schema_version: u16,
            _value: &Value,
        ) -> Result<Option<ReminderSection>, CompactionStateError> {
            Ok(None)
        }
    }

    fn tool_message(path: &str, undone: bool) -> Message {
        Message {
            role: Role::Tool,
            content: vec![ContentBlock::ToolResult(ToolResultBlock {
                id: format!("call-{path}"),
                name: "edit".to_string(),
                output: vec![ContentBlock::text("edited")],
                state: ToolResultState::Success,
                artifacts: vec![ToolResultArtifact {
                    kind: "file_change".to_string(),
                    payload: json!({
                        "changeId": format!("change-{path}"),
                        "path": path,
                        "kind": "modified",
                        "additions": 1,
                        "deletions": 1,
                        "hunks": [],
                        "beforeHash": "before",
                        "afterHash": "after",
                        "undone": undone
                    }),
                }],
            })],
        }
    }

    #[tokio::test]
    async fn collects_active_file_changes_and_renders_stably() {
        let collector = CompactionStateCollector::default();
        let messages = vec![
            tool_message("b.rs", false),
            tool_message("a.rs", false),
            tool_message("ignored.rs", true),
            tool_message("a.rs", false),
        ];

        let (state, reminder) = collector.collect(&messages).await.expect("state");

        assert_eq!(state.edited_paths, ["a.rs", "b.rs"]);
        assert_eq!(
            reminder,
            "<system_reminder format_version=\"1\">\n## Edited paths\n- a.rs\n- b.rs\n</system_reminder>"
        );
    }

    #[tokio::test]
    async fn renders_a_stable_empty_reminder() {
        let collector = CompactionStateCollector::default();
        let (state, reminder) = collector.collect(&[]).await.expect("state");

        assert!(state.edited_paths.is_empty());
        assert_eq!(
            reminder,
            "<system_reminder format_version=\"1\">\nNo additional durable runtime state was recorded at compaction time.\n</system_reminder>"
        );
    }

    #[tokio::test]
    async fn carries_forward_unknown_extensions_and_rederives_file_state() {
        let collector = CompactionStateCollector::default();
        let mut base = CompactionRuntimeState {
            edited_paths: vec!["a.rs".to_string(), "b.rs".to_string()],
            ..CompactionRuntimeState::default()
        };
        base.extensions.insert(
            "future_state".to_string(),
            CompactionStateEntry {
                schema_version: 7,
                value: json!({ "opaque": true }),
            },
        );

        let (state, reminder) = collector
            .collect_with_base(
                &[
                    tool_message("a.rs", true),
                    tool_message("b.rs", false),
                    tool_message("c.rs", false),
                ],
                base,
            )
            .await
            .expect("state");

        assert_eq!(state.edited_paths, ["b.rs", "c.rs"]);
        assert_eq!(state.extensions["future_state"].schema_version, 7);
        assert_eq!(
            state.extensions["future_state"].value,
            json!({ "opaque": true })
        );
        assert_eq!(
            reminder,
            "<system_reminder format_version=\"1\">\n## Edited paths\n- b.rs\n- c.rs\n</system_reminder>"
        );
    }

    #[tokio::test]
    async fn refreshes_bounded_warnings_and_rejects_unknown_root_versions() {
        let collector = CompactionStateCollector::default()
            .with_contributor(Box::new(FailingBestEffortContributor))
            .expect("contributor");
        let mut base = CompactionRuntimeState::default();
        base.warnings.push(CompactionStateWarning {
            contributor_key: "stale".to_string(),
            code: "old".to_string(),
        });

        let (state, _) = collector
            .collect_with_base(&[], base)
            .await
            .expect("best effort state");
        assert_eq!(
            state.warnings,
            [CompactionStateWarning {
                contributor_key: "future_best_effort".to_string(),
                code: "collection_failed".to_string(),
            }]
        );

        let unsupported = CompactionRuntimeState {
            schema_version: 2,
            ..CompactionRuntimeState::default()
        };
        assert!(matches!(
            collector.collect_with_base(&[], unsupported).await,
            Err(CompactionStateError::UnsupportedRuntimeStateVersion(2))
        ));
    }
}
