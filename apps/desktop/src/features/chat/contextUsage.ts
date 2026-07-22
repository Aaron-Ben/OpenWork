import type { RuntimeTurnTrace } from '../../bridge/compat'

export interface ContextUsage {
  usedTokens: number
  totalTokens: number
  estimated: boolean
}

export function contextUsageFromTrace(
  trace: RuntimeTurnTrace,
  totalTokens: number,
): ContextUsage | null {
  const normalizedTotal = positiveInteger(totalTokens)
  if (normalizedTotal === null) return null

  const latestModelCall = trace.spans
    .filter((span) => span.kind === 'model_call')
    .reduce<(typeof trace.spans)[number] | null>(
      (latest, span) => latest === null || span.sequence > latest.sequence ? span : latest,
      null,
    )
  if (!latestModelCall) return null

  const providerTokens = nonNegativeInteger(latestModelCall.inputTokens)
  const estimatedTokens = nonNegativeInteger(
    latestModelCall.attributes.requestEstimatedInputTokens,
  )
  const usedTokens = providerTokens ?? estimatedTokens
  if (usedTokens === null) return null

  return {
    usedTokens,
    totalTokens: normalizedTotal,
    estimated: providerTokens === null,
  }
}

function positiveInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value > 0
    ? value
    : null
}

function nonNegativeInteger(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
    ? value
    : null
}
