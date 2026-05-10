import { describe, expect, it } from 'vitest'

import { createMockRun } from '$lib/test-utils/factories'
import {
  type ColIdx,
  type Columns,
  clampRow,
  locate,
  nextNonEmptyColumn,
  type Position,
  resolveTarget,
  runIdAt,
} from './geometry'

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

// 10-card queued column (indices 0–9)
const queued = Array.from({ length: 10 }, (_, i) =>
  createMockRun({ id: BigInt(100 + i), status: 'Queued' }),
)

// 3-card inProgress column (indices 0–2)
const inProgress = Array.from({ length: 3 }, (_, i) =>
  createMockRun({ id: BigInt(200 + i), status: 'InProgress' }),
)

// 5-card completed column (indices 0–4)
const completed = Array.from({ length: 5 }, (_, i) =>
  createMockRun({ id: BigInt(300 + i), status: 'Completed' }),
)

// Standard 3-column setup: 10 | 3 | 5
const threeCol: Columns = [queued, inProgress, completed] as const

// For skip-empty tests: [5 queued, 0 inProgress, 3 completed]
const queued5 = Array.from({ length: 5 }, (_, i) =>
  createMockRun({ id: BigInt(400 + i), status: 'Queued' }),
)
const completed3 = Array.from({ length: 3 }, (_, i) =>
  createMockRun({ id: BigInt(500 + i), status: 'Completed' }),
)
const skipMiddle: Columns = [queued5, [], completed3] as const

// For skip-and-no-further test: [3 queued, 0 inProgress, 0 completed]
const queued3 = Array.from({ length: 3 }, (_, i) =>
  createMockRun({ id: BigInt(600 + i), status: 'Queued' }),
)
const noRightNeighbour: Columns = [queued3, [], []] as const

// Fully empty columns
const allEmpty: Columns = [[], [], []] as const

// ---------------------------------------------------------------------------
// locate()
// ---------------------------------------------------------------------------

describe('locate', () => {
  it('returns null when runId is null', () => {
    const result = locate(null, threeCol)
    expect(result).toBeNull()
  })

  it('returns null when runId is not in any column', () => {
    const result = locate(999n, threeCol)
    expect(result).toBeNull()
  })

  it('finds a run in column 0', () => {
    const expected: Position = { col: 0 as ColIdx, row: 3 }
    expect(locate(queued[3]!.id, threeCol)).toEqual(expected)
  })

  it('finds a run in column 1', () => {
    const expected: Position = { col: 1 as ColIdx, row: 1 }
    expect(locate(inProgress[1]!.id, threeCol)).toEqual(expected)
  })

  it('finds a run in column 2', () => {
    const expected: Position = { col: 2 as ColIdx, row: 4 }
    expect(locate(completed[4]!.id, threeCol)).toEqual(expected)
  })
})

// ---------------------------------------------------------------------------
// nextNonEmptyColumn()
// ---------------------------------------------------------------------------

describe('nextNonEmptyColumn', () => {
  it('returns null when moving right from rightmost non-empty col (2)', () => {
    expect(nextNonEmptyColumn(2, 1, threeCol)).toBeNull()
  })

  it('returns null when moving left from leftmost non-empty col (0)', () => {
    expect(nextNonEmptyColumn(0, -1, threeCol)).toBeNull()
  })

  it('returns adjacent column when it is non-empty (right)', () => {
    expect(nextNonEmptyColumn(0, 1, threeCol)).toBe(1)
  })

  it('returns adjacent column when it is non-empty (left)', () => {
    expect(nextNonEmptyColumn(2, -1, threeCol)).toBe(1)
  })

  it('skips an empty middle column moving right', () => {
    expect(nextNonEmptyColumn(0, 1, skipMiddle)).toBe(2)
  })

  it('skips an empty middle column moving left', () => {
    expect(nextNonEmptyColumn(2, -1, skipMiddle)).toBe(0)
  })

  it('returns null when no further non-empty column exists (right)', () => {
    expect(nextNonEmptyColumn(0, 1, noRightNeighbour)).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// clampRow()
// ---------------------------------------------------------------------------

describe('clampRow', () => {
  it('returns desiredRow unchanged when within bounds', () => {
    expect(clampRow(1, 1, threeCol)).toBe(1)
  })

  it('clamps to last index when desiredRow exceeds column length', () => {
    // inProgress has 3 cards; desiredRow 5 → 2
    expect(clampRow(1, 5, threeCol)).toBe(2)
  })

  it('clamps to 0 when desiredRow is negative', () => {
    expect(clampRow(0, -1, threeCol)).toBe(0)
  })

  it('returns 0 when desiredRow is 0 (boundary)', () => {
    expect(clampRow(0, 0, threeCol)).toBe(0)
  })
})

// ---------------------------------------------------------------------------
// runIdAt()
// ---------------------------------------------------------------------------

describe('runIdAt', () => {
  it('returns the run id at a valid position', () => {
    const pos: Position = { col: 0 as ColIdx, row: 2 }
    expect(runIdAt(pos, threeCol)).toBe(queued[2]!.id)
  })

  it('returns null for an out-of-bounds row', () => {
    const pos: Position = { col: 1 as ColIdx, row: 99 }
    expect(runIdAt(pos, threeCol)).toBeNull()
  })

  it('returns null when column is empty', () => {
    const pos: Position = { col: 1 as ColIdx, row: 0 }
    expect(runIdAt(pos, allEmpty)).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// resolveTarget() — ArrowDown
// ---------------------------------------------------------------------------

describe('resolveTarget / ArrowDown', () => {
  it('moves to row+1 in the same column (non-edge)', () => {
    const current = queued[3]!.id
    const result = resolveTarget(current, 'ArrowDown', threeCol)
    expect(result).toBe(queued[4]!.id)
  })

  it('last row of column is a no-op — returns same id', () => {
    const last = queued[queued.length - 1]!
    const result = resolveTarget(last.id, 'ArrowDown', threeCol)
    expect(result).toBe(last.id)
  })
})

// ---------------------------------------------------------------------------
// resolveTarget() — ArrowUp
// ---------------------------------------------------------------------------

describe('resolveTarget / ArrowUp', () => {
  it('moves to row-1 in the same column (non-edge)', () => {
    const current = queued[4]!.id
    const result = resolveTarget(current, 'ArrowUp', threeCol)
    expect(result).toBe(queued[3]!.id)
  })

  it('first row (row 0) is a no-op — returns same id', () => {
    const first = queued[0]!
    const result = resolveTarget(first.id, 'ArrowUp', threeCol)
    expect(result).toBe(first.id)
  })
})

// ---------------------------------------------------------------------------
// resolveTarget() — ArrowRight
// ---------------------------------------------------------------------------

describe('resolveTarget / ArrowRight', () => {
  it('moves to adjacent non-empty column with matching row', () => {
    // queued[1] → inProgress[1]
    const result = resolveTarget(queued[1]!.id, 'ArrowRight', threeCol)
    expect(result).toBe(inProgress[1]!.id)
  })

  it('ArrowRight clamps row when target column is shorter', () => {
    // queued has 10 cards, inProgress has 3; row 5 → clamped to 2
    expect(resolveTarget(queued[5]!.id, 'ArrowRight', threeCol)).toBe(inProgress[2]!.id)
  })

  it('rightmost non-empty column is a no-op — returns same id', () => {
    const last = completed[0]!
    const result = resolveTarget(last.id, 'ArrowRight', threeCol)
    expect(result).toBe(last.id)
  })

  it('skips empty middle column when moving right', () => {
    // skipMiddle = [5 queued, 0 inProgress, 3 completed]
    const result = resolveTarget(queued5[1]!.id, 'ArrowRight', skipMiddle)
    expect(result).toBe(completed3[1]!.id)
  })

  it('empty middle col with no further col is a no-op — returns same id', () => {
    // noRightNeighbour = [3 queued, 0, 0]
    const result = resolveTarget(queued3[1]!.id, 'ArrowRight', noRightNeighbour)
    expect(result).toBe(queued3[1]!.id)
  })
})

// ---------------------------------------------------------------------------
// resolveTarget() — ArrowLeft
// ---------------------------------------------------------------------------

describe('resolveTarget / ArrowLeft', () => {
  it('moves to adjacent non-empty column with matching row', () => {
    // inProgress[1] → queued[1]
    const result = resolveTarget(inProgress[1]!.id, 'ArrowLeft', threeCol)
    expect(result).toBe(queued[1]!.id)
  })

  it('leftmost non-empty column is a no-op — returns same id', () => {
    const first = queued[2]!
    const result = resolveTarget(first.id, 'ArrowLeft', threeCol)
    expect(result).toBe(first.id)
  })

  it('skips empty middle column when moving left', () => {
    // skipMiddle = [5 queued, 0 inProgress, 3 completed]
    const result = resolveTarget(completed3[1]!.id, 'ArrowLeft', skipMiddle)
    expect(result).toBe(queued5[1]!.id)
  })
})

// ---------------------------------------------------------------------------
// resolveTarget() — Home / End
// ---------------------------------------------------------------------------

describe('resolveTarget / Home', () => {
  it('moves to row 0 of the same column', () => {
    const result = resolveTarget(queued[7]!.id, 'Home', threeCol)
    expect(result).toBe(queued[0]!.id)
  })

  it('Home at row 0 is a no-op — returns same id', () => {
    const result = resolveTarget(queued[0]!.id, 'Home', threeCol)
    expect(result).toBe(queued[0]!.id)
  })
})

describe('resolveTarget / End', () => {
  it('moves to last row of the same column', () => {
    const result = resolveTarget(queued[2]!.id, 'End', threeCol)
    expect(result).toBe(queued[queued.length - 1]!.id)
  })

  it('End at last row is a no-op — returns same id', () => {
    const last = queued[queued.length - 1]!
    const result = resolveTarget(last.id, 'End', threeCol)
    expect(result).toBe(last.id)
  })
})

// ---------------------------------------------------------------------------
// resolveTarget() — null/eviction fallback cases
// ---------------------------------------------------------------------------

describe('resolveTarget / null and eviction cases', () => {
  it('currentRunId null with all columns empty → returns null', () => {
    const result = resolveTarget(null, 'ArrowDown', allEmpty)
    expect(result).toBeNull()
  })

  it('currentRunId null with queued having cards → returns first queued id', () => {
    const result = resolveTarget(null, 'ArrowDown', threeCol)
    expect(result).toBe(queued[0]!.id)
  })

  it('currentRunId null with only completed having cards → returns first completed id', () => {
    const onlyCompleted: Columns = [[], [], completed] as const
    const result = resolveTarget(null, 'ArrowDown', onlyCompleted)
    expect(result).toBe(completed[0]!.id)
  })

  it('currentRunId not in any column (eviction) → returns null', () => {
    const result = resolveTarget(9999n, 'ArrowDown', threeCol)
    expect(result).toBeNull()
  })
})
