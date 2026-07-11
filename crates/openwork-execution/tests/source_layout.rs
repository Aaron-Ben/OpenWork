use std::path::Path;

#[test]
fn action_handlers_are_grouped_by_execution_domain() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    for path in [
        "actions/mod.rs",
        "actions/output.rs",
        "actions/filesystem/mod.rs",
        "actions/filesystem/read.rs",
        "actions/filesystem/write.rs",
        "actions/filesystem/edit.rs",
        "actions/filesystem/list.rs",
        "actions/filesystem/grep.rs",
        "actions/filesystem/glob.rs",
        "actions/process/mod.rs",
        "actions/process/bash.rs",
    ] {
        assert!(src.join(path).is_file(), "missing action module: {path}");
    }

    assert!(
        !src.join("builtin").exists(),
        "legacy builtin directory must be removed"
    );

    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("read execution lib.rs");
    assert!(lib.contains("mod actions;"));
    assert!(!lib.contains("mod builtin;"));

    let invoker = std::fs::read_to_string(src.join("invoker.rs")).expect("read invoker.rs");
    assert!(invoker.contains("crate::actions"));
    assert!(!invoker.contains("crate::builtin"));
}
