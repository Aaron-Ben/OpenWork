use openwork_protocol::{
    approval::{ApprovalPolicy, ExecutionPolicyDecision},
    capability::CapabilityRiskHint,
};

pub(crate) fn evaluate(
    policy: ApprovalPolicy,
    risk_hint: CapabilityRiskHint,
) -> ExecutionPolicyDecision {
    match policy {
        ApprovalPolicy::Never => ExecutionPolicyDecision::Allow,
        ApprovalPolicy::Untrusted => ExecutionPolicyDecision::RequireApproval {
            reason: approval_reason(risk_hint).to_string(),
        },
    }
}

fn approval_reason(risk_hint: CapabilityRiskHint) -> &'static str {
    match risk_hint {
        CapabilityRiskHint::ReadOnly => "read-only action requires user approval",
        CapabilityRiskHint::WorkspaceMutation => "workspace mutation requires user approval",
        CapabilityRiskHint::ProcessExecution => "process execution requires user approval",
    }
}
