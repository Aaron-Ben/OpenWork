/**
 * Agent 识别色（collaboration-desktop.md §11）：按 Agent ID 的稳定哈希取 6 档之一，不随列表顺序变化。
 * 只用于头像、名字、提及标签和工作中外圈。
 */

export interface IdentityClasses {
  text: string
  avatar: string
  ring: string
  mention: string
}

// Tailwind 只收录源码里出现的完整类名，所以 6 档逐一写出，不拼接。
const TONES: [IdentityClasses, ...IdentityClasses[]] = [
  { text: 'text-agent-1', avatar: 'bg-agent-1/15 text-agent-1', ring: 'ring-agent-1', mention: 'bg-agent-1/15 text-agent-1' },
  { text: 'text-agent-2', avatar: 'bg-agent-2/15 text-agent-2', ring: 'ring-agent-2', mention: 'bg-agent-2/15 text-agent-2' },
  { text: 'text-agent-3', avatar: 'bg-agent-3/15 text-agent-3', ring: 'ring-agent-3', mention: 'bg-agent-3/15 text-agent-3' },
  { text: 'text-agent-4', avatar: 'bg-agent-4/15 text-agent-4', ring: 'ring-agent-4', mention: 'bg-agent-4/15 text-agent-4' },
  { text: 'text-agent-5', avatar: 'bg-agent-5/15 text-agent-5', ring: 'ring-agent-5', mention: 'bg-agent-5/15 text-agent-5' },
  { text: 'text-agent-6', avatar: 'bg-agent-6/15 text-agent-6', ring: 'ring-agent-6', mention: 'bg-agent-6/15 text-agent-6' },
]

/** FNV-1a 32 位哈希取模：同一个 id 永远落在同一档。 */
export function identityIndex(agentId: string): number {
  let hash = 0x811c9dc5
  for (const character of agentId) {
    hash ^= character.codePointAt(0) ?? 0
    hash = Math.imul(hash, 0x01000193) >>> 0
  }
  return hash % TONES.length
}

export function identityClasses(agentId: string): IdentityClasses {
  return TONES[identityIndex(agentId)] ?? TONES[0]
}
