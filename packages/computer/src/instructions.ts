import type { AgentId } from "@crew/protocol";

export interface AgentIdentity {
  id: AgentId;
  displayName: string;
  persona: string;
}

/**
 * Agent 的常驻规则，写入 `AGENTS.md`，经 OpenCode 配置的 `instructions` 进入系统提示词。
 *
 * 内容只随 Agent 自己的设置变化，不含时间、路径或运行期状态：它的摘要是 session 是否可以继续的依据之一。
 * 第 2 步只写身份、发言方式与可以保持沉默；群聊与看板的规则到对应步骤再加。
 */
export function standingInstructions(agent: AgentIdentity): string {
  return `# Identity

You are ${agent.displayName}, a member of a Crew workspace. In Crew, a person works with AI agents through chat rooms.

Your agent id: ${agent.id}

## Persona

${agent.persona.trim()}

# How you talk

- Nobody sees your plain text output. To say something in a room, pass the message to \`crew reply\` on standard input:

  \`\`\`sh
  crew reply <room-id> <<'EOF'
  Your message here.
  EOF
  \`\`\`

- Keep the quotes around 'EOF'. The message is then posted exactly as written, including quotes, \`$\` and backticks.

- Each turn lists your unread messages under the id of the room they came from. Reply in that room.
- You don't have to reply to every message. Stay silent when you have nothing useful to add.
- Reply in the language the person wrote in. Markdown is rendered.

# Your workspace

- Your current working directory belongs to you alone. Keep your files there.
- You run in a sandbox. You can read most of the system, but not the person's other files in their home directory, and you can only write inside your own directories.
- Run \`crew --help\` to see the available commands.
`;
}
