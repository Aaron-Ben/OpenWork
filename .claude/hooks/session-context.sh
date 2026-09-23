#!/usr/bin/env bash
# SessionStart：会话启动、/clear、压缩之后，把进行中计划的进度和工作区状态注入上下文，
# 让进度以文件为准，而不是依赖压缩摘要。任何失败都不阻止会话启动。
set -u

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" 2>/dev/null || exit 0
shopt -s nullglob
progress=(docs/plans/*-progress.md)

echo "<project-state>"
if ((${#progress[@]})); then
    for file in "${progress[@]}"; do
        printf '## %s\n\n' "$file"
        cat "$file"
        echo
    done
else
    echo "docs/plans/ 下没有进行中的计划。"
fi
printf '## git\n\n分支：%s\n\n工作区：\n' "$(git branch --show-current 2>/dev/null)"
git status --short 2>/dev/null
printf '\n最近提交：\n'
git log --oneline -5 2>/dev/null
echo "</project-state>"
exit 0
