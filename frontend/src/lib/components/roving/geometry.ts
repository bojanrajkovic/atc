import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type ColIdx = 0 | 1 | 2

export type Position = { col: ColIdx; row: number }

export type Columns = readonly [
  readonly WorkflowRun[],
  readonly WorkflowRun[],
  readonly WorkflowRun[],
]

export type ArrowKey = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight' | 'Home' | 'End'

// ---------------------------------------------------------------------------
// locate() — O(n) linear scan across all columns
// ---------------------------------------------------------------------------

/**
 * Find the position of a run in the columns tuple.
 * Returns null if runId is null or not present in any column.
 */
export function locate(runId: bigint | null, columns: Columns): Position | null {
  if (runId === null) return null

  for (const col of [0, 1, 2] as const) {
    const column = columns[col]
    for (let row = 0; row < column.length; row++) {
      if (column[row]?.id === runId) {
        return { col, row }
      }
    }
  }

  return null
}

// ---------------------------------------------------------------------------
// nextNonEmptyColumn()
// ---------------------------------------------------------------------------

/**
 * Returns the first column index in the given direction whose array is non-empty.
 * Returns null if no such column exists.
 */
export function nextNonEmptyColumn(from: ColIdx, dir: -1 | 1, columns: Columns): ColIdx | null {
  let c = from + dir
  while (c >= 0 && c <= 2) {
    const colIdx = c as ColIdx
    if (columns[colIdx].length > 0) {
      return colIdx
    }
    c += dir
  }
  return null
}

// ---------------------------------------------------------------------------
// clampRow()
// ---------------------------------------------------------------------------

/**
 * Clamps desiredRow into [0, columns[targetCol].length - 1].
 * Caller guarantees the target column is non-empty.
 */
export function clampRow(targetCol: ColIdx, desiredRow: number, columns: Columns): number {
  const max = columns[targetCol].length - 1
  if (desiredRow < 0) return 0
  if (desiredRow > max) return max
  return desiredRow
}

// ---------------------------------------------------------------------------
// runIdAt()
// ---------------------------------------------------------------------------

/**
 * Returns the run id at the given position, or null if out of bounds.
 */
export function runIdAt(pos: Position, columns: Columns): bigint | null {
  return columns[pos.col][pos.row]?.id ?? null
}

// ---------------------------------------------------------------------------
// resolveTarget()
// ---------------------------------------------------------------------------

/**
 * The orchestrator. Returns the target run id given the current focus and
 * the arrow key.
 *
 * - Returns null for the null-current-all-empty and eviction cases.
 * - Returns the SAME id for no-op edges (e.g., ArrowDown at last row).
 * - Returns a different id when movement succeeds.
 */
export function resolveTarget(
  currentRunId: bigint | null,
  key: ArrowKey,
  columns: Columns,
): bigint | null {
  // Null-current fallback: return the first card of the first non-empty column.
  if (currentRunId === null) {
    for (const col of [0, 1, 2] as const) {
      const first = columns[col][0]
      if (first !== undefined) {
        return first.id
      }
    }
    return null
  }

  // Eviction case: run is gone from columns.
  const pos = locate(currentRunId, columns)
  if (pos === null) return null

  switch (key) {
    case 'ArrowDown': {
      const newRow = pos.row + 1
      if (newRow >= columns[pos.col].length) {
        // No-op: already at last row.
        return currentRunId
      }
      return runIdAt({ col: pos.col, row: newRow }, columns)
    }

    case 'ArrowUp': {
      if (pos.row === 0) {
        // No-op: already at first row.
        return currentRunId
      }
      return runIdAt({ col: pos.col, row: pos.row - 1 }, columns)
    }

    case 'ArrowRight': {
      const targetCol = nextNonEmptyColumn(pos.col, 1, columns)
      if (targetCol === null) {
        // No-op: rightmost non-empty column.
        return currentRunId
      }
      const row = clampRow(targetCol, pos.row, columns)
      return runIdAt({ col: targetCol, row }, columns)
    }

    case 'ArrowLeft': {
      const targetCol = nextNonEmptyColumn(pos.col, -1, columns)
      if (targetCol === null) {
        // No-op: leftmost non-empty column.
        return currentRunId
      }
      const row = clampRow(targetCol, pos.row, columns)
      return runIdAt({ col: targetCol, row }, columns)
    }

    case 'Home': {
      if (pos.row === 0) {
        return currentRunId
      }
      return runIdAt({ col: pos.col, row: 0 }, columns)
    }

    case 'End': {
      const lastRow = columns[pos.col].length - 1
      if (pos.row === lastRow) {
        return currentRunId
      }
      return runIdAt({ col: pos.col, row: lastRow }, columns)
    }
  }
}
