export type CommandErrorCode =
  | 'invalid_request'
  | 'provider_not_found'
  | 'session_not_found'
  | 'turn_not_found'
  | 'approval_not_found'
  | 'database_unavailable'
  | 'schema_not_ready'
  | 'configuration_invalid'
  | 'operation_conflict'
  | 'model_request_failed'
  | 'internal_error'

export interface CommandError {
  code: CommandErrorCode
  message: string
}

const COMMAND_ERROR_CODES = new Set<CommandErrorCode>([
  'invalid_request',
  'provider_not_found',
  'session_not_found',
  'turn_not_found',
  'approval_not_found',
  'database_unavailable',
  'schema_not_ready',
  'configuration_invalid',
  'operation_conflict',
  'model_request_failed',
  'internal_error',
])

export function resolveCommandError(error: unknown): CommandError {
  if (isCommandError(error)) return error
  if (error instanceof Error) return { code: 'internal_error', message: error.message }
  if (typeof error === 'string') return { code: 'internal_error', message: error }
  return { code: 'internal_error', message: 'Unexpected error' }
}

export function resolveErrorMessage(error: unknown): string {
  return resolveCommandError(error).message
}

function isCommandError(error: unknown): error is CommandError {
  if (!error || typeof error !== 'object') return false
  const candidate = error as Partial<CommandError>
  return (
    typeof candidate.code === 'string' &&
    COMMAND_ERROR_CODES.has(candidate.code as CommandErrorCode) &&
    typeof candidate.message === 'string'
  )
}
