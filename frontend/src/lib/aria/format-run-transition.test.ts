import { describe, expect, it } from 'vitest'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { formatRunTransition } from './format-run-transition'
import type { TransitionKind } from './transition-kinds'

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeRun(overrides: Partial<WorkflowRun> = {}): WorkflowRun {
  return {
    id: 1n,
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: 'Fix stuff',
    event: 'push',
    displayTitle: 'Fix the thing',
    status: 'Queued',
    conclusion: null,
    htmlUrl: 'https://example.com',
    createdAt: new Date().toISOString(),
    runStartedAt: null,
    updatedAt: new Date().toISOString(),
    runAttempt: 1,
    ...overrides,
  }
}

const queued: TransitionKind = { kind: 'queued' }
const succeeded: TransitionKind = { kind: 'completed', conclusion: 'Success' }
const failed: TransitionKind = { kind: 'completed', conclusion: 'Failure' }
const cancelled: TransitionKind = { kind: 'completed', conclusion: 'Cancelled' }
const timedOut: TransitionKind = { kind: 'completed', conclusion: 'TimedOut' }

// ---------------------------------------------------------------------------
// Per-kind message format
// ---------------------------------------------------------------------------

describe('formatRunTransition', () => {
  describe('queued transition', () => {
    it('formats a queued message with all fields', () => {
      const run = makeRun({
        displayTitle: 'Deploy app',
        org: 'acme',
        repo: 'backend',
        branch: 'main',
        event: 'push',
      })
      const msg = formatRunTransition(run, queued)
      expect(msg).toBe('Run Deploy app for acme/backend on main (push) queued')
    })

    it('uses "queued" as the verb for Requested transitions', () => {
      const run = makeRun()
      const msg = formatRunTransition(run, queued)
      expect(msg).toContain('queued')
    })
  })

  describe('completed transition', () => {
    it('formats a succeeded message', () => {
      const run = makeRun({
        displayTitle: 'Build',
        org: 'org',
        repo: 'repo',
        branch: 'feature',
        event: 'push',
      })
      const msg = formatRunTransition(run, succeeded)
      expect(msg).toBe('Run Build for org/repo on feature (push) succeeded')
    })

    it('formats a failed message', () => {
      const run = makeRun({
        displayTitle: 'Tests',
        org: 'org',
        repo: 'repo',
        branch: 'main',
        event: 'pull_request',
      })
      const msg = formatRunTransition(run, failed)
      expect(msg).toBe('Run Tests for org/repo on main (pull_request) failed')
    })

    it('formats a cancelled message', () => {
      const run = makeRun()
      const msg = formatRunTransition(run, cancelled)
      expect(msg).toContain('cancelled')
    })

    it('formats a timed out message', () => {
      const run = makeRun()
      const msg = formatRunTransition(run, timedOut)
      expect(msg).toContain('timed out')
    })

    it('uses conclusion-specific verbs from VERB_BY_CONCLUSION for all 9 variants', () => {
      const run = makeRun()
      const cases: Array<{
        conclusion: TransitionKind & { kind: 'completed' }
        expectedVerb: string
      }> = [
        { conclusion: { kind: 'completed', conclusion: 'Success' }, expectedVerb: 'succeeded' },
        { conclusion: { kind: 'completed', conclusion: 'Failure' }, expectedVerb: 'failed' },
        { conclusion: { kind: 'completed', conclusion: 'Cancelled' }, expectedVerb: 'cancelled' },
        { conclusion: { kind: 'completed', conclusion: 'TimedOut' }, expectedVerb: 'timed out' },
        {
          conclusion: { kind: 'completed', conclusion: 'ActionRequired' },
          expectedVerb: 'requires action',
        },
        { conclusion: { kind: 'completed', conclusion: 'Stale' }, expectedVerb: 'went stale' },
        {
          conclusion: { kind: 'completed', conclusion: 'Neutral' },
          expectedVerb: 'completed neutral',
        },
        { conclusion: { kind: 'completed', conclusion: 'Skipped' }, expectedVerb: 'was skipped' },
        {
          conclusion: { kind: 'completed', conclusion: 'StartupFailure' },
          expectedVerb: 'failed to start',
        },
      ]
      for (const { conclusion, expectedVerb } of cases) {
        const msg = formatRunTransition(run, conclusion)
        expect(msg).toContain(expectedVerb)
      }
    })
  })

  // ---------------------------------------------------------------------------
  // Branch elision when null
  // ---------------------------------------------------------------------------

  describe('branch elision', () => {
    it('elides "on {branch}" when branch is null', () => {
      const run = makeRun({ branch: null })
      const msg = formatRunTransition(run, queued)
      expect(msg).not.toContain('on null')
      expect(msg).not.toContain(' on ')
      expect(msg).toContain('queued')
    })

    it('includes the branch when present', () => {
      const run = makeRun({ branch: 'feature/foo' })
      const msg = formatRunTransition(run, queued)
      expect(msg).toContain('on feature/foo')
    })

    it('branch elision also works for completed transitions', () => {
      const run = makeRun({ branch: null })
      const msg = formatRunTransition(run, succeeded)
      expect(msg).not.toContain(' on ')
      expect(msg).toContain('succeeded')
    })
  })

  // ---------------------------------------------------------------------------
  // Message structure
  // ---------------------------------------------------------------------------

  describe('message structure', () => {
    it('starts with "Run {displayTitle}"', () => {
      const run = makeRun({ displayTitle: 'My workflow' })
      const msg = formatRunTransition(run, queued)
      expect(msg.startsWith('Run My workflow')).toBe(true)
    })

    it('includes "for {org}/{repo}"', () => {
      const run = makeRun({ org: 'my-org', repo: 'my-repo' })
      const msg = formatRunTransition(run, queued)
      expect(msg).toContain('for my-org/my-repo')
    })

    it('includes "({event})" in parentheses', () => {
      const run = makeRun({ event: 'schedule' })
      const msg = formatRunTransition(run, queued)
      expect(msg).toContain('(schedule)')
    })
  })
})
