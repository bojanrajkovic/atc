import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

/** Branded canonical identifier for a runner pool. Sort-and-join of label array. */
export type PoolKey = string & { readonly __brand: 'PoolKey' }

/** Build a PoolKey from a label array. Order-independent, idempotent. */
export function poolKey(labels: readonly string[]): PoolKey {
  return [...labels].sort().join('|') as PoolKey
}

/**
 * Runtime validator for a `PoolKey` deserialized from an untrusted source
 * (URL state, storage, etc.). Returns the branded value on canonical input,
 * `null` on anything else. The brand is compile-time only, so any boundary
 * crossing this module must funnel through `parsePoolKey` to preserve the
 * invariant that callers never see a fake-branded string.
 *
 * Canonical form: non-empty `'|'`-separated segments in ascending sort order
 * with no empty segments. `parsePoolKey(poolKey(x)) === poolKey(x)` for every
 * non-empty `x`.
 */
export function parsePoolKey(s: string): PoolKey | null {
  if (s === '') return null
  const parts = s.split('|')
  if (parts.some((p) => p === '')) return null
  for (let i = 1; i < parts.length; i++) {
    if (parts[i]! < parts[i - 1]!) return null
  }
  return s as PoolKey
}

/** True iff every label in poolLabels is present in jobLabels (intersection check). */
export function jobMatchesPool(
  jobLabels: readonly string[],
  poolLabels: readonly string[],
): boolean {
  return poolLabels.every((label) => jobLabels.includes(label))
}

/**
 * Filter runs to those whose jobs include ALL of the pool's labels (subset match).
 * A job matches if its label set is a superset of the pool's labels — a job can
 * carry extra labels (e.g., capabilities) beyond what the pool key encodes.
 * When poolFilter is null, returns runs unchanged (identity passthrough).
 *
 * NOTE: the runs argument is the readonly array consumers already pass to
 * KanbanColumn. We do not look up jobs from a store — jobsByRunId comes in
 * as a prop so this module stays pure.
 */
export function filterRunsByPool(
  runs: readonly WorkflowRun[],
  jobsByRunId: ReadonlyMap<bigint, readonly Job[]>,
  poolFilter: PoolKey | null,
): readonly WorkflowRun[] {
  if (poolFilter === null) return runs
  return runs.filter((run) => {
    const jobs = jobsByRunId.get(run.id) ?? []
    return jobs.some((job) =>
      jobMatchesPool(job.labels, poolFilter.split('|') as readonly string[]),
    )
  })
}
