use crate::{ToolDefinition, ToolRisk};
use serde_json::{Value, json};

pub(crate) fn builtin_definitions() -> Vec<ToolDefinition> {
    vec![
        spec(
            "read",
            "Read a UTF-8 text file from the filesystem. Returns content prefixed with line numbers. Refuses files larger than 1 MB.",
            json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path, or a path relative to the working directory."
                    }
                },
                "required": ["path"]
            }),
            ToolRisk::ReadOnly,
        ),
        spec(
            "write",
            "Write text content to a file. Creates the file (and parent directories) if missing; overwrites if it exists.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute or working-dir-relative path." },
                    "content": { "type": "string", "description": "Full file content to write." }
                },
                "required": ["path", "content"]
            }),
            ToolRisk::WorkspaceMutation,
        ),
        spec(
            "edit",
            "Edit a file by replacing a unique occurrence of `oldString` with `newString`. Use `oldString: \"\"` to create a new file (refuses if it already exists). Set `replaceAll: true` to replace every occurrence. Without `replaceAll`, `oldString` must match exactly and be unique in the file.",
            json!({
                "type": "object",
                "properties": {
                    "filePath": { "type": "string", "description": "Absolute or working-dir-relative path." },
                    "oldString": { "type": "string", "description": "Exact text to find. Empty string means create a new file." },
                    "newString": { "type": "string", "description": "Replacement text (or full content for a new file)." },
                    "replaceAll": { "type": "boolean", "description": "Replace every occurrence. Defaults to false." }
                },
                "required": ["filePath", "oldString", "newString"]
            }),
            ToolRisk::WorkspaceMutation,
        ),
        spec(
            "grep",
            "Search file contents with a regular expression (ripgrep-like; respects .gitignore). Returns `path:line:content` by default, just file paths in `files_with_matches` mode, or `path:count` in `count` mode. Use `glob` to filter file types (e.g. \"*.rs\").",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regular expression to search for." },
                    "path": { "type": "string", "description": "Directory or file to search; defaults to the working directory." },
                    "glob": { "type": "string", "description": "Optional glob to filter files, e.g. \"*.rs\"." },
                    "outputMode": { "type": "string", "enum": ["content", "files_with_matches", "count"], "description": "Defaults to \"content\"." },
                    "maxResults": { "type": "number", "description": "Max results to return. Defaults to 200." }
                },
                "required": ["pattern"]
            }),
            ToolRisk::ReadOnly,
        ),
        spec(
            "glob",
            "Find files by name pattern (e.g. \"**/*.rs\"). Respects .gitignore. Returns matching file paths, one per line.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern, e.g. \"**/*.rs\" or \"src/**/*.ts\"." },
                    "path": { "type": "string", "description": "Directory to search; defaults to the working directory." }
                },
                "required": ["pattern"]
            }),
            ToolRisk::ReadOnly,
        ),
        spec(
            "list",
            "List entries in a directory. Directories are suffixed with '/'. Defaults to the working directory.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Directory path; defaults to the working directory." }
                }
            }),
            ToolRisk::ReadOnly,
        ),
        spec(
            "bash",
            "Run a shell command via `sh -c` in the working directory. Returns combined stdout/stderr and the exit code. Subject to approval.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute." },
                    "timeoutMs": { "type": "number", "description": "Optional timeout in milliseconds. Defaults to 30000 and is capped at 120000." }
                },
                "required": ["command"]
            }),
            ToolRisk::ProcessExecution,
        ),
    ]
}

fn spec(name: &str, description: &str, input_schema: Value, risk_hint: ToolRisk) -> ToolDefinition {
    ToolDefinition {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
        risk_hint,
    }
}
