#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arity {
    None,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandKind {
    AllPaths,
    PatternThenPaths,
    NoPaths,
    None,
}

pub(crate) struct ReadonlyCommand {
    pub(crate) key: &'static str,
    pub(crate) flags: &'static [(&'static str, Arity)],
    pub(crate) operands: OperandKind,
    pub(crate) max_operands: Option<usize>,
}

const fn command(
    key: &'static str,
    flags: &'static [(&'static str, Arity)],
    operands: OperandKind,
) -> ReadonlyCommand {
    ReadonlyCommand {
        key,
        flags,
        operands,
        max_operands: None,
    }
}

const fn bounded_command(
    key: &'static str,
    flags: &'static [(&'static str, Arity)],
    operands: OperandKind,
    max_operands: usize,
) -> ReadonlyCommand {
    ReadonlyCommand {
        key,
        flags,
        operands,
        max_operands: Some(max_operands),
    }
}

const LS_FLAGS: &[(&str, Arity)] = &[
    ("-a", Arity::None),
    ("-A", Arity::None),
    ("-h", Arity::None),
    ("-l", Arity::None),
    ("-R", Arity::None),
    ("--all", Arity::None),
    ("--almost-all", Arity::None),
    ("--human-readable", Arity::None),
    ("--recursive", Arity::None),
    ("--color", Arity::None),
    ("--color=always", Arity::None),
    ("--color=auto", Arity::None),
    ("--color=never", Arity::None),
];

const CAT_FLAGS: &[(&str, Arity)] = &[
    ("-A", Arity::None),
    ("-b", Arity::None),
    ("-e", Arity::None),
    ("-E", Arity::None),
    ("-n", Arity::None),
    ("-s", Arity::None),
    ("-t", Arity::None),
    ("-T", Arity::None),
    ("-u", Arity::None),
    ("-v", Arity::None),
    ("--number-nonblank", Arity::None),
    ("--show-all", Arity::None),
    ("--show-ends", Arity::None),
    ("--number", Arity::None),
    ("--squeeze-blank", Arity::None),
    ("--show-tabs", Arity::None),
    ("--show-nonprinting", Arity::None),
];

const HEAD_TAIL_FLAGS: &[(&str, Arity)] = &[
    ("-c", Arity::One),
    ("-n", Arity::One),
    ("-q", Arity::None),
    ("-v", Arity::None),
    ("--bytes", Arity::One),
    ("--lines", Arity::One),
    ("--quiet", Arity::None),
    ("--silent", Arity::None),
    ("--verbose", Arity::None),
];

const WC_FLAGS: &[(&str, Arity)] = &[
    ("-c", Arity::None),
    ("-l", Arity::None),
    ("-m", Arity::None),
    ("-w", Arity::None),
    ("-L", Arity::None),
    ("--bytes", Arity::None),
    ("--chars", Arity::None),
    ("--lines", Arity::None),
    ("--max-line-length", Arity::None),
    ("--words", Arity::None),
];

const NL_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::One),
    ("-d", Arity::One),
    ("-f", Arity::One),
    ("-h", Arity::One),
    ("-i", Arity::One),
    ("-l", Arity::One),
    ("-n", Arity::One),
    ("-p", Arity::None),
    ("-s", Arity::One),
    ("-v", Arity::One),
    ("-w", Arity::One),
];

const TAC_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::None),
    ("-r", Arity::None),
    ("-s", Arity::One),
    ("--before", Arity::None),
    ("--regex", Arity::None),
    ("--separator", Arity::One),
];

const RG_FLAGS: &[(&str, Arity)] = &[
    ("-c", Arity::None),
    ("-F", Arity::None),
    ("-i", Arity::None),
    ("-n", Arity::None),
    ("--count", Arity::None),
    ("--fixed-strings", Arity::None),
    ("--ignore-case", Arity::None),
    ("--line-number", Arity::None),
];

const GREP_FLAGS: &[(&str, Arity)] = &[
    ("-c", Arity::None),
    ("-E", Arity::None),
    ("-F", Arity::None),
    ("-G", Arity::None),
    ("-H", Arity::None),
    ("-h", Arity::None),
    ("-i", Arity::None),
    ("-l", Arity::None),
    ("-L", Arity::None),
    ("-n", Arity::None),
    ("-o", Arity::None),
    ("-q", Arity::None),
    ("-s", Arity::None),
    ("-v", Arity::None),
    ("-w", Arity::None),
    ("-x", Arity::None),
    ("--count", Arity::None),
    ("--extended-regexp", Arity::None),
    ("--fixed-strings", Arity::None),
    ("--ignore-case", Arity::None),
    ("--line-number", Arity::None),
    ("--quiet", Arity::None),
    ("--recursive", Arity::None),
    ("--invert-match", Arity::None),
    ("--word-regexp", Arity::None),
];

const REALPATH_FLAGS: &[(&str, Arity)] = &[
    ("-e", Arity::None),
    ("-L", Arity::None),
    ("-m", Arity::None),
    ("-P", Arity::None),
    ("-q", Arity::None),
    ("-s", Arity::None),
    ("-z", Arity::None),
    ("--canonicalize-existing", Arity::None),
    ("--canonicalize-missing", Arity::None),
    ("--logical", Arity::None),
    ("--physical", Arity::None),
    ("--quiet", Arity::None),
    ("--strip", Arity::None),
    ("--zero", Arity::None),
];

const STAT_FLAGS: &[(&str, Arity)] = &[
    ("-L", Arity::None),
    ("-t", Arity::None),
    ("--dereference", Arity::None),
    ("--terse", Arity::None),
    ("-c", Arity::One),
    ("--format", Arity::One),
    ("--printf", Arity::One),
];

const FILE_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::None),
    ("-h", Arity::None),
    ("-i", Arity::None),
    ("-I", Arity::None),
    ("-k", Arity::None),
    ("-L", Arity::None),
    ("-s", Arity::None),
    ("--brief", Arity::None),
    ("--dereference", Arity::None),
    ("--keep-going", Arity::None),
    ("--mime", Arity::None),
    ("--mime-type", Arity::None),
    ("--special-files", Arity::None),
];

const SORT_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::None),
    ("-f", Arity::None),
    ("-h", Arity::None),
    ("-n", Arity::None),
    ("-r", Arity::None),
    ("-s", Arity::None),
    ("-u", Arity::None),
    ("-k", Arity::One),
    ("-t", Arity::One),
    ("--ignore-leading-blanks", Arity::None),
    ("--ignore-case", Arity::None),
    ("--human-numeric-sort", Arity::None),
    ("--numeric-sort", Arity::None),
    ("--reverse", Arity::None),
    ("--stable", Arity::None),
    ("--unique", Arity::None),
    ("--key", Arity::One),
    ("--field-separator", Arity::One),
];

const UNIQ_FLAGS: &[(&str, Arity)] = &[
    ("-c", Arity::None),
    ("-d", Arity::None),
    ("-i", Arity::None),
    ("-u", Arity::None),
    ("-f", Arity::One),
    ("-s", Arity::One),
    ("-w", Arity::One),
    ("--count", Arity::None),
    ("--repeated", Arity::None),
    ("--ignore-case", Arity::None),
    ("--unique", Arity::None),
    ("--skip-fields", Arity::One),
    ("--skip-chars", Arity::One),
    ("--check-chars", Arity::One),
];

const CUT_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::One),
    ("-c", Arity::One),
    ("-d", Arity::One),
    ("-f", Arity::One),
    ("-s", Arity::None),
    ("--bytes", Arity::One),
    ("--characters", Arity::One),
    ("--delimiter", Arity::One),
    ("--fields", Arity::One),
    ("--only-delimited", Arity::None),
];

const DIFF_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::None),
    ("-B", Arity::None),
    ("-i", Arity::None),
    ("-q", Arity::None),
    ("-r", Arity::None),
    ("-s", Arity::None),
    ("-u", Arity::None),
    ("-w", Arity::None),
    ("-U", Arity::One),
    ("--brief", Arity::None),
    ("--ignore-all-space", Arity::None),
    ("--ignore-blank-lines", Arity::None),
    ("--ignore-case", Arity::None),
    ("--recursive", Arity::None),
    ("--report-identical-files", Arity::None),
    ("--unified", Arity::One),
];

const UNAME_FLAGS: &[(&str, Arity)] = &[
    ("-a", Arity::None),
    ("-m", Arity::None),
    ("-n", Arity::None),
    ("-p", Arity::None),
    ("-r", Arity::None),
    ("-s", Arity::None),
    ("-v", Arity::None),
];

const HOSTNAME_FLAGS: &[(&str, Arity)] = &[("-f", Arity::None), ("-s", Arity::None)];

const DF_DU_FLAGS: &[(&str, Arity)] = &[
    ("-a", Arity::None),
    ("-h", Arity::None),
    ("-k", Arity::None),
    ("-P", Arity::None),
    ("-s", Arity::None),
    ("--all", Arity::None),
    ("--human-readable", Arity::None),
    ("--summarize", Arity::None),
];

const GIT_STATUS_FLAGS: &[(&str, Arity)] = &[
    ("-b", Arity::None),
    ("-s", Arity::None),
    ("--branch", Arity::None),
    ("--long", Arity::None),
    ("--porcelain", Arity::None),
    ("--short", Arity::None),
    ("--show-stash", Arity::None),
];

const GIT_DIFF_FLAGS: &[(&str, Arity)] = &[
    ("-G", Arity::One),
    ("-S", Arity::One),
    ("--cached", Arity::None),
    ("--color", Arity::None),
    ("--name-only", Arity::None),
    ("--name-status", Arity::None),
    ("--no-color", Arity::None),
    ("--stat", Arity::None),
];

const GIT_HISTORY_FLAGS: &[(&str, Arity)] = &[
    ("-G", Arity::One),
    ("-S", Arity::One),
    ("-n", Arity::One),
    ("--all", Arity::None),
    ("--decorate", Arity::None),
    ("--format", Arity::One),
    ("--grep", Arity::One),
    ("--max-count", Arity::One),
    ("--name-only", Arity::None),
    ("--name-status", Arity::None),
    ("--oneline", Arity::None),
    ("--pretty", Arity::One),
    ("--stat", Arity::None),
];

const GIT_BLAME_FLAGS: &[(&str, Arity)] = &[
    ("-L", Arity::One),
    ("-w", Arity::None),
    ("--line-porcelain", Arity::None),
    ("--porcelain", Arity::None),
    ("--show-email", Arity::None),
];

const GIT_LS_FILES_FLAGS: &[(&str, Arity)] = &[
    ("-c", Arity::None),
    ("-d", Arity::None),
    ("-m", Arity::None),
    ("-o", Arity::None),
    ("-s", Arity::None),
    ("--cached", Arity::None),
    ("--deleted", Arity::None),
    ("--modified", Arity::None),
    ("--others", Arity::None),
    ("--stage", Arity::None),
];

const GIT_REV_PARSE_FLAGS: &[(&str, Arity)] = &[
    ("--abbrev-ref", Arity::None),
    ("--is-inside-work-tree", Arity::None),
    ("--show-prefix", Arity::None),
    ("--show-toplevel", Arity::None),
    ("--verify", Arity::None),
];

const GIT_WORKTREE_LIST_FLAGS: &[(&str, Arity)] =
    &[("--porcelain", Arity::None), ("--verbose", Arity::None)];

pub(crate) const COMMANDS: &[ReadonlyCommand] = &[
    command("git stash list", GIT_HISTORY_FLAGS, OperandKind::NoPaths),
    command(
        "git worktree list",
        GIT_WORKTREE_LIST_FLAGS,
        OperandKind::None,
    ),
    command("git status", GIT_STATUS_FLAGS, OperandKind::AllPaths),
    command("git diff", GIT_DIFF_FLAGS, OperandKind::AllPaths),
    command("git log", GIT_HISTORY_FLAGS, OperandKind::NoPaths),
    command("git show", GIT_HISTORY_FLAGS, OperandKind::NoPaths),
    command("git blame", GIT_BLAME_FLAGS, OperandKind::AllPaths),
    command("git ls-files", GIT_LS_FILES_FLAGS, OperandKind::AllPaths),
    command("git rev-parse", GIT_REV_PARSE_FLAGS, OperandKind::NoPaths),
    command("git shortlog", GIT_HISTORY_FLAGS, OperandKind::NoPaths),
    command("git merge-base", &[], OperandKind::NoPaths),
    command("git describe", &[], OperandKind::NoPaths),
    command("pwd", &[], OperandKind::None),
    command("ls", LS_FLAGS, OperandKind::AllPaths),
    command("cat", CAT_FLAGS, OperandKind::AllPaths),
    command("head", HEAD_TAIL_FLAGS, OperandKind::AllPaths),
    command("tail", HEAD_TAIL_FLAGS, OperandKind::AllPaths),
    command("wc", WC_FLAGS, OperandKind::AllPaths),
    command("nl", NL_FLAGS, OperandKind::AllPaths),
    command("tac", TAC_FLAGS, OperandKind::AllPaths),
    command("rev", &[], OperandKind::AllPaths),
    command("rg", RG_FLAGS, OperandKind::PatternThenPaths),
    command("grep", GREP_FLAGS, OperandKind::PatternThenPaths),
    bounded_command("basename", &[], OperandKind::AllPaths, 2),
    command("dirname", &[], OperandKind::AllPaths),
    command("realpath", REALPATH_FLAGS, OperandKind::AllPaths),
    command("stat", STAT_FLAGS, OperandKind::AllPaths),
    command("file", FILE_FLAGS, OperandKind::AllPaths),
    command("sort", SORT_FLAGS, OperandKind::AllPaths),
    bounded_command("uniq", UNIQ_FLAGS, OperandKind::AllPaths, 1),
    command("cut", CUT_FLAGS, OperandKind::AllPaths),
    bounded_command("tr", &[], OperandKind::NoPaths, 2),
    command("diff", DIFF_FLAGS, OperandKind::AllPaths),
    command("whoami", &[], OperandKind::None),
    command("id", &[], OperandKind::NoPaths),
    command("uname", UNAME_FLAGS, OperandKind::None),
    command("hostname", HOSTNAME_FLAGS, OperandKind::None),
    command("date", &[], OperandKind::None),
    command("df", DF_DU_FLAGS, OperandKind::AllPaths),
    command("du", DF_DU_FLAGS, OperandKind::AllPaths),
    command("which", &[], OperandKind::NoPaths),
    command("type", &[], OperandKind::NoPaths),
    command("echo", &[], OperandKind::NoPaths),
    command("printf", &[], OperandKind::NoPaths),
    command("true", &[], OperandKind::None),
    command("false", &[], OperandKind::None),
    command("seq", &[], OperandKind::NoPaths),
];
