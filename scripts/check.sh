#!/usr/bin/env bash
# 完整检查：全部通过才算完成（docs/plans 各计划 §4.1）。
# 每一步都会跑完，最后汇总失败的步骤，避免一处失败掩盖其他问题。
set -uo pipefail

cd "$(dirname "$0")/.."

if [[ -z "${TEST_DATABASE_URL:-}" ]]; then
    echo "TEST_DATABASE_URL 未设置：postgres 测试会静默跳过，结果不可信。" >&2
    echo "例如：TEST_DATABASE_URL=postgres://openwork:openwork@127.0.0.1:5432/openwork_test scripts/check.sh" >&2
    exit 1
fi

failed=()

step() {
    local name=$1
    shift
    printf '\n==> %s\n' "$name"
    "$@" || failed+=("$name")
}

step "cargo fmt" cargo fmt --all -- --check
step "cargo clippy" cargo clippy --workspace --all-targets -- -D warnings
step "cargo test" cargo test --workspace --no-fail-fast
step "desktop typecheck" pnpm --dir desktop typecheck
step "desktop test" pnpm --dir desktop test

if ((${#failed[@]})); then
    printf '\n失败：%s\n' "${failed[*]}" >&2
    exit 1
fi
printf '\n全部检查通过。\n'
