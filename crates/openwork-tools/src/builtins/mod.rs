mod filesystem;
mod output;
mod process;

pub(crate) use output::truncate_output;

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

    use crate::{PermissionProfile, ToolSessionContext, ToolsetConfig};

    use super::*;

    #[test]
    fn builtin_registry_exposes_each_tool_once_in_selected_order() {
        let names = ["read", "write", "edit", "grep", "glob", "list", "bash"];
        let toolset = builtin_registry()
            .finalize(
                &ToolsetConfig::from_names(names),
                ToolSessionContext::local(
                    std::env::temp_dir(),
                    PermissionProfile::danger_full_access(),
                ),
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
            ("write", &["path", "content"][..]),
            (
                "edit",
                &["filePath", "oldString", "newString", "replaceAll"][..],
            ),
            (
                "grep",
                &["pattern", "path", "glob", "outputMode", "maxResults"][..],
            ),
            ("glob", &["pattern", "path", "maxResults"][..]),
            ("list", &["path", "offset", "limit"][..]),
            ("bash", &["command", "timeoutMs"][..]),
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
}
