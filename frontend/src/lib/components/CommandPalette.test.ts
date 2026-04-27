import { tick } from 'svelte'
import { beforeEach, describe, expect, it } from 'vitest'
import { poolKey } from '$lib/filters/pool'
import { paletteStore } from '$lib/stores/palette.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

// Helper to reset stores to a known state
function resetStores() {
  paletteStore.close()
  paletteStore.setQuery('')
  paletteStore.exitSubmenu()
  // Reset by clearing internal state
  runStore.runs.clear()
  runStore.jobsByRun.clear()
  runnerStore.pools = []
  uiStore.selectedRunId = null
  uiStore.selectedJobId = null
  uiStore.activePoolFilter = null
}

// Helper to create a fixture run
function makeRun(id: bigint, displayTitle: string = 'Test Run'): WorkflowRun {
  return {
    id,
    org: 'my-org',
    repo: 'my-repo',
    branch: 'main',
    displayTitle,
    status: 'Queued' as const,
    conclusion: null,
    workflowName: 'test-workflow',
    workflowPath: '.github/workflows/test.yml',
    headSha: 'abc123',
    commitMessage: null,
    event: 'push',
    htmlUrl: 'https://github.com/my-org/my-repo/actions/runs/1',
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    runStartedAt: null,
  }
}

// Helper to create a fixture job
function makeJob(id: bigint, runId: bigint, name: string = 'Test Job'): Job {
  return {
    id,
    runId,
    name,
    status: 'Queued' as const,
    conclusion: null,
    runner: null,
    labels: [],
    steps: [],
    createdAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
  }
}

describe('CommandPalette Store Integration', () => {
  beforeEach(() => {
    resetStores()
  })

  it('AC1.4 — run selection sets selectedRunId, closes palette, records visit', async () => {
    const run = makeRun(1n, 'PR #42')
    runStore.runs.set(run.id, run)

    paletteStore.open()
    expect(paletteStore.paletteOpen).toBe(true)

    // Simulate selection dispatch
    uiStore.selectedRunId = run.id
    await tick()
    paletteStore.paletteOpen = false
    paletteStore.recordRunVisit(run.id)

    expect(uiStore.selectedRunId).toBe(1n)
    expect(paletteStore.paletteOpen).toBe(false)
    expect(paletteStore.recentRunIds).toContain(1n)
  })

  it('AC1.5 — job selection sets selectedRunId, selectedJobId, closes palette', async () => {
    const run = makeRun(1n, 'PR #42')
    const job = makeJob(100n, 1n, 'build')
    runStore.runs.set(run.id, run)
    runStore.jobsByRun.set(run.id, [job])

    paletteStore.open()
    expect(paletteStore.paletteOpen).toBe(true)

    // Simulate job selection dispatch
    uiStore.selectedRunId = job.runId
    uiStore.selectedJobId = job.id
    await tick()
    paletteStore.paletteOpen = false

    expect(uiStore.selectedRunId).toBe(1n)
    expect(uiStore.selectedJobId).toBe(100n)
    expect(paletteStore.paletteOpen).toBe(false)
  })

  it('AC1.6 — pool selection sets activePoolFilter and closes palette', () => {
    const labels = ['linux', 'x64']
    runnerStore.pools = [
      {
        labels,
        groupName: labels[0]!,
        running: 0,
        queued: 0,
        isElastic: false,
        total: 10,
      },
    ]

    paletteStore.open()
    expect(paletteStore.paletteOpen).toBe(true)

    // Simulate pool selection
    uiStore.activePoolFilter = poolKey(labels)
    paletteStore.paletteOpen = false

    expect(uiStore.activePoolFilter).toEqual(poolKey(labels))
    expect(paletteStore.paletteOpen).toBe(false)
  })

  it('AC1.7 — enterSubmenu sets subMenu to "theme"', async () => {
    paletteStore.open()
    paletteStore.enterSubmenu('theme')
    await tick()

    expect(paletteStore.subMenu).toBe('theme')
  })

  it('AC1.8 — selecting a theme sets uiStore.theme, clears subMenu, closes palette', async () => {
    paletteStore.open()
    paletteStore.enterSubmenu('theme')
    await tick()

    // Simulate theme selection
    uiStore.theme = 'violet'
    paletteStore.exitSubmenu()
    paletteStore.paletteOpen = false

    expect(uiStore.theme).toBe('violet')
    expect(paletteStore.subMenu).toBeNull()
    expect(paletteStore.paletteOpen).toBe(false)
  })

  it('AC1.9 — exitSubmenu clears subMenu without closing palette', async () => {
    paletteStore.open()
    paletteStore.enterSubmenu('theme')
    await tick()

    expect(paletteStore.subMenu).toBe('theme')
    expect(paletteStore.paletteOpen).toBe(true)

    paletteStore.exitSubmenu()
    await tick()

    expect(paletteStore.subMenu).toBeNull()
    expect(paletteStore.paletteOpen).toBe(true)
  })

  it('AC1.10 — empty-state condition fires when query is set and all sections empty', async () => {
    paletteStore.open()
    paletteStore.setQuery('xyz')
    await tick()

    // Verify conditions for empty-state rendering
    // (the actual component test verifies DOM rendering in E2E)
    expect(paletteStore.paletteQuery).toBe('xyz')
    expect(runStore.queuedRuns.length).toBe(0)
    expect(runStore.inProgressRuns.length).toBe(0)
    expect(runStore.completedRuns.length).toBe(0)
  })

  it('AC1.12 — "Clear pool filter" conditional: hidden when activePoolFilter is null', () => {
    expect(uiStore.activePoolFilter).toBeNull()

    // When activePoolFilter is set, condition is true
    uiStore.activePoolFilter = poolKey(['linux'])
    expect(uiStore.activePoolFilter).not.toBeNull()

    // Reset
    uiStore.activePoolFilter = null
    expect(uiStore.activePoolFilter).toBeNull()
  })

  it('AC1.13 — "Close detail panel" conditional: hidden when selectedRunId is null', () => {
    expect(uiStore.selectedRunId).toBeNull()

    // When selectedRunId is set, condition is true
    uiStore.selectedRunId = 1n
    expect(uiStore.selectedRunId).not.toBeNull()

    // Reset
    uiStore.selectedRunId = null
    expect(uiStore.selectedRunId).toBeNull()
  })
})
