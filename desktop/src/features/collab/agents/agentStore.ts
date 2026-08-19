import { create } from 'zustand'

import {
  collabCommands,
  type CollabAgent,
  type CollabAgentActivity,
  type CollabAgentInput,
} from '@/bridge/collab'
import { resolveErrorMessage } from '@/lib/commandError'

interface AgentStoreState {
  agents: CollabAgent[]
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  create: (input: CollabAgentInput) => Promise<void>
  update: (input: CollabAgentInput) => Promise<void>
  applyActivity: (agentId: string, activity: CollabAgentActivity) => void
}

export const useAgentStore = create<AgentStoreState>((set, get) => ({
  agents: [],
  loading: false,
  error: null,
  fetchAll: async () => {
    set({ loading: true, error: null })
    try {
      set({ agents: await collabCommands.listAgents(), loading: false })
    } catch (error) {
      set({ error: resolveErrorMessage(error), loading: false })
    }
  },
  create: async (input) => {
    await collabCommands.createAgent(input)
    await get().fetchAll()
  },
  update: async (input) => {
    await collabCommands.updateAgent(input)
    await get().fetchAll()
  },
  applyActivity: (agentId, activity) => {
    set({
      agents: get().agents.map((agent) =>
        agent.id === agentId ? { ...agent, activity } : agent,
      ),
    })
  },
}))
