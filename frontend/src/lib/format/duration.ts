export type DurationSpec =
  | { kind: 'static'; startMs: number; endMs: number }
  | { kind: 'live'; startMs: number; nowMs: number }

export function parseIso(iso: string): number {
  return new Date(iso).getTime()
}

export function formatDuration(spec: DurationSpec): string {
  const diffMs = spec.kind === 'static' ? spec.endMs - spec.startMs : spec.nowMs - spec.startMs

  if (diffMs < 0) return '0:00'

  const totalSeconds = Math.floor(diffMs / 1000)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60

  const ss = seconds.toString().padStart(2, '0')

  if (hours > 0) {
    const mm = minutes.toString().padStart(2, '0')
    return `${hours}:${mm}:${ss}`
  }

  return `${minutes}:${ss}`
}
