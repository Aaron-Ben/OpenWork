/**
 * Preview of the daemon's agent-id derivation (docs/collaboration.md §3.1).
 *
 * The daemon re-derives authoritatively at save time — this mirror only powers
 * the read-only preview in the create form, so it must stay in sync with
 * `crates/openwork-collab/src/identity.rs` (mirrored tests in agentId.test.ts).
 */

/** Longest id accepted by the `collab_participants` CHECK constraint. */
export const MAX_AGENT_ID_LENGTH = 48

export function deriveAgentSlug(name: string): string | null {
  let slug = ''
  for (const character of name.trim()) {
    if ((character >= 'a' && character <= 'z') || (character >= '0' && character <= '9')) {
      slug += character
    } else if (character >= 'A' && character <= 'Z') {
      slug += character.toLowerCase()
    } else if (character === ' ' || character === '-' || character === '_') {
      if (!slug.endsWith('_')) slug += '_'
    }
  }
  const base = slug
    .replace(/^_+|_+$/g, '')
    .replace(/^[0-9]+/, '')
    .replace(/^_+|_+$/g, '')
    .slice(0, MAX_AGENT_ID_LENGTH)
    .replace(/_+$/, '')
  return base.length > 0 ? base : null
}
