import type { Job } from '$lib/types/generated/Job'

export function summarizeRunners(jobs: readonly Job[]): string | null {
  const names = new Set<string>()
  for (const job of jobs) {
    if (job.runner !== null) names.add(job.runner.name)
  }
  if (names.size === 0) return null
  if (names.size === 1) {
    const [only] = names
    // only is narrowed to string | undefined by noUncheckedIndexedAccess;
    // we know size === 1 so only is defined, but the idiomatic guard is:
    return only ?? null
  }
  return `${names.size} runners`
}
