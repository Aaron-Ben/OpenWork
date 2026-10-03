mod filesystem;
mod process;

use filesystem::{EditTool, GlobTool, GrepTool, ListTool, ReadTool, WriteTool};
use process::BashTool;

use crate::ToolRegistryBuilder;

pub fn builtin_registry() -> ToolRegistryBuilder {
    ToolRegistryBuilder::new()
        .register(ReadTool)
        .register(WriteTool)
        .register(EditTool)
        .register(GrepTool)
        .register(GlobTool)
        .register(ListTool)
        .register(BashTool)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use std::sync::Arc;

    use openwork_sandbox::{SandboxBackend, Seatbelt};

    use crate::{ToolSessionContext, ToolsetConfig};

    use super::*;

    #[test]
    fn builtin_registry_exposes_each_tool_once_in_selected_order() {
        let names = ["read", "write", "edit", "grep", "glob", "list", "bash"];
        let toolset = builtin_registry()
            .finalize(
                &ToolsetConfig::from_names(names),
                crate::test_support::unconfined_session(&std::env::temp_dir()),
            )
            .expect("builtin toolset");

        assert_eq!(
            toolset
                .definitions()
                .iter()
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            names
        );

        let expected_properties = [
            ("read", &["path", "offset", "limit"][..]),
            (
                "write",
                &["path", "content", "sandboxPermissions", "justification"][..],
            ),
            (
                "edit",
                &[
                    "filePath",
                    "oldString",
                    "newString",
                    "replaceAll",
                    "sandboxPermissions",
                    "justification",
                ][..],
            ),
            ("grep", &["pattern", "path", "glob", "outputMode"][..]),
            ("glob", &["pattern", "path"][..]),
            ("list", &["path", "offset", "limit"][..]),
            (
                "bash",
                &[
                    "command",
                    "timeoutMs",
                    "sandboxPermissions",
                    "justification",
                ][..],
            ),
        ];
        for (name, properties) in expected_properties {
            let definition = toolset.resolve(name).expect("builtin definition");
            assert!(!definition.description.is_empty());
            let actual = definition.input_schema["properties"]
                .as_object()
                .expect("object properties")
                .keys()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            assert_eq!(actual, properties.iter().copied().collect());
        }
    }

    /// permissions.md §9.2、tools.md §10 #10f：沙箱不可用时 schema 里没有越界参数。
    #[test]
    fn escalation_parameters_disappear_when_the_sandbox_is_unavailable() {
        let unavailable: Arc<dyn SandboxBackend> =
            Arc::new(Seatbelt::probe("/nonexistent/sandbox-exec"));
        let toolset = builtin_registry()
            .finalize(
                &ToolsetConfig::from_names(["write", "edit", "bash"]),
                ToolSessionContext::local(std::env::temp_dir(), unavailable),
            )
            .expect("builtin toolset");
        for name in ["write", "edit", "bash"] {
            let schema = &toolset.resolve(name).expect("definition").input_schema;
            let text = schema.to_string();
            assert!(!text.contains("sandboxPermissions"), "{name}: {text}");
            assert!(!text.contains("justification"), "{name}: {text}");
            assert!(schema.get("$defs").is_none(), "{name}: {text}");
        }
    }
}
