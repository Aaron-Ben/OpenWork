#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Arity {
    None,
    One,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OperandKind {
    AllPaths,
    PatternThenPaths,
    None,
}

pub(crate) struct ReadonlyCommand {
    pub(crate) key: &'static str,
    pub(crate) flags: &'static [(&'static str, Arity)],
    pub(crate) operands: OperandKind,
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

const RG_FLAGS: &[(&str, Arity)] = &[
    ("-F", Arity::None),
    ("-i", Arity::None),
    ("-n", Arity::None),
    ("--fixed-strings", Arity::None),
    ("--ignore-case", Arity::None),
    ("--line-number", Arity::None),
];

const GIT_DIFF_FLAGS: &[(&str, Arity)] = &[
    ("-S", Arity::One),
    ("--name-only", Arity::None),
    ("--name-status", Arity::None),
    ("--stat", Arity::None),
];

pub(crate) const COMMANDS: &[ReadonlyCommand] = &[
    ReadonlyCommand {
        key: "git status",
        flags: &[],
        operands: OperandKind::None,
    },
    ReadonlyCommand {
        key: "git diff",
        flags: GIT_DIFF_FLAGS,
        operands: OperandKind::None,
    },
    ReadonlyCommand {
        key: "pwd",
        flags: &[],
        operands: OperandKind::None,
    },
    ReadonlyCommand {
        key: "ls",
        flags: LS_FLAGS,
        operands: OperandKind::AllPaths,
    },
    ReadonlyCommand {
        key: "cat",
        flags: CAT_FLAGS,
        operands: OperandKind::AllPaths,
    },
    ReadonlyCommand {
        key: "rg",
        flags: RG_FLAGS,
        operands: OperandKind::PatternThenPaths,
    },
];
