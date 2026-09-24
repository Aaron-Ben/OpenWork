//! 沙箱相关的模型可见文本（permissions.md §4.6）：拒绝标记与越界提示。
//!
//! 标记在决策点上指出边界，而不是事先写进系统提示词。措辞逐字由测试钉住，
//! 改这里要同时改对应断言（agent-context.md §5）。

use openwork_sandbox::{Access, SandboxMode};

/// 模型与界面上使用的模式名。
pub(crate) fn mode_name(mode: SandboxMode) -> &'static str {
    match mode {
        SandboxMode::AcceptEdits => "accept-edits",
        SandboxMode::Auto => "auto",
    }
}

/// 追加在被内核拒绝的 bash 结果末尾。`escalation_available` 为假时（沙箱不可用、或
/// 调用方不能请求越界）不提越界参数，免得模型去用一个不存在的选项。
pub(crate) fn bash_denied(mode: SandboxMode, escalation_available: bool) -> String {
    let mut text = format!(
        "[sandbox: file access denied under {} mode]",
        mode_name(mode)
    );
    if escalation_available {
        text.push_str(
            "\n[sandbox: to proceed, retry this exact command once with sandboxPermissions listing only the paths it needs, and a one-sentence justification; the user will be asked. If the paths cannot be listed, ask the user to run the command instead]",
        );
    }
    text
}

/// 文件工具的路径被当前模式拒绝（敏感、凭据、工作区外）。
pub(crate) fn file_denied(
    display: &str,
    access: Access,
    mode: SandboxMode,
    escalation_available: bool,
) -> String {
    let access = access_name(access);
    let mut text = format!(
        "[sandbox: {access} access to {display} denied under {} mode]",
        mode_name(mode)
    );
    if escalation_available {
        text.push_str(&format!(
            "\n[sandbox: to proceed, retry this call once with sandboxPermissions listing {display} ({access}, exact), and a one-sentence justification; the user will be asked]"
        ));
    }
    text
}

/// 硬保护路径：任何模式、任何越界下都不可写。
pub(crate) fn protected(display: &str) -> String {
    format!("[sandbox: {display} is protected and cannot be written in any mode; do not retry]")
}

fn access_name(access: Access) -> &'static str {
    match access {
        Access::Read => "read",
        Access::Write => "write",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_denial_names_the_mode_and_offers_escalation_only_when_available() {
        assert_eq!(
            bash_denied(SandboxMode::AcceptEdits, false),
            "[sandbox: file access denied under accept-edits mode]"
        );
        assert!(bash_denied(SandboxMode::Auto, true).contains("sandboxPermissions"));
    }

    #[test]
    fn file_denial_names_the_path_and_the_exact_grant_to_ask_for() {
        assert_eq!(
            file_denied(".env", Access::Write, SandboxMode::Auto, true),
            "[sandbox: write access to .env denied under auto mode]\n[sandbox: to proceed, retry this call once with sandboxPermissions listing .env (write, exact), and a one-sentence justification; the user will be asked]"
        );
    }
}
