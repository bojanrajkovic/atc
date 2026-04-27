import type { StatusKey } from './status-key'

/**
 * Convert a StatusKey to its corresponding CSS variable name suffix.
 * Used for styling status indicators with --{result} CSS custom property.
 */
export function statusKeyToVar(key: StatusKey): string {
  switch (key) {
    case 'Queued':
      return 'queued'
    case 'InProgress':
      return 'running'
    case 'Success':
      return 'success'
    case 'Failure':
      return 'failed'
    case 'Cancelled':
      return 'cancelled'
    case 'TimedOut':
      return 'timed-out'
    case 'ActionRequired':
      return 'action-required'
    case 'StartupFailure':
      return 'failed'
    case 'Stale':
      return 'neutral'
    case 'Neutral':
      return 'neutral'
    case 'Skipped':
      return 'neutral'
  }
}
