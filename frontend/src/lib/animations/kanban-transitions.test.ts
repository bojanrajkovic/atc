import { describe, expect, it, vi } from 'vitest'

function mockMatchMedia(reducedMotion: boolean) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query === '(prefers-reduced-motion: reduce)' ? reducedMotion : false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }))
}

describe('kanban-transitions module', () => {
  describe('kanban-board.AC5.1: Animation module exports the expected contract', () => {
    it('exports send, receive, and duration constants', async () => {
      // Clear module cache to test fresh import
      vi.resetModules()

      // Mock normal reduce preference (false)
      mockMatchMedia(false)

      // Import the module
      const { send, receive, DURATION_MOVE, DURATION_ARRIVE, DURATION_REMOVE, FLY_SETTLE_Y } =
        await import('$lib/animations/kanban-transitions')

      // Assert all exports are defined (not undefined)
      expect(send).toBeDefined()
      expect(receive).toBeDefined()
      expect(DURATION_MOVE).toBeDefined()
      expect(DURATION_ARRIVE).toBeDefined()
      expect(DURATION_REMOVE).toBeDefined()
      expect(FLY_SETTLE_Y).toBeDefined()

      // Assert send and receive are functions
      expect(typeof send).toBe('function')
      expect(typeof receive).toBe('function')

      // Assert numeric constants are numbers
      expect(typeof DURATION_MOVE).toBe('number')
      expect(typeof DURATION_ARRIVE).toBe('number')
      expect(typeof DURATION_REMOVE).toBe('number')
      expect(typeof FLY_SETTLE_Y).toBe('number')

      // Assert non-zero for normal motion (not reduced)
      expect(DURATION_MOVE).toBeGreaterThan(0)
      expect(DURATION_ARRIVE).toBeGreaterThan(0)
      expect(DURATION_REMOVE).toBeGreaterThan(0)
    })
  })

  describe('kanban-board.AC5.2: Crossfade fallback returns a TransitionConfig', () => {
    it('send without matching receive uses fallback with intro=false (fade)', async () => {
      // Clear module cache
      vi.resetModules()

      // Mock normal reduce preference
      mockMatchMedia(false)

      const { send, DURATION_REMOVE } = await import('$lib/animations/kanban-transitions')

      // Create a minimal DOM element
      const node = document.createElement('div')

      // Call send with a key - when there's no matching receive, the fallback is used with intro=false
      const transitionFn = send(node, { key: 'test-key' })

      // send returns a function
      expect(typeof transitionFn).toBe('function')

      // Invoke the function to get the actual TransitionConfig
      const config = transitionFn()

      // The fallback with intro=false should produce a fade-like config with DURATION_REMOVE
      expect(config).toBeDefined()
      expect(config.duration).toBe(DURATION_REMOVE)
    })

    it('receive without matching send uses fallback with intro=true (fly)', async () => {
      // Clear module cache
      vi.resetModules()

      // Mock normal reduce preference
      mockMatchMedia(false)

      const { receive, DURATION_ARRIVE } = await import('$lib/animations/kanban-transitions')

      // Create a minimal DOM element
      const node = document.createElement('div')

      // Call receive with a key - when there's no matching send, the fallback is used with intro=true
      const transitionFn = receive(node, { key: 'test-key' })

      // receive returns a function
      expect(typeof transitionFn).toBe('function')

      // Invoke the function to get the actual TransitionConfig
      const config = transitionFn()

      // The fallback with intro=true should produce a fly-like config with DURATION_ARRIVE
      expect(config).toBeDefined()
      expect(config.duration).toBe(DURATION_ARRIVE)
    })
  })

  describe('kanban-board.AC6: Animations respect prefers-reduced-motion', () => {
    describe('AC6.1 & AC6.2: Durations are zero when reduced motion is true', () => {
      it('sets all durations to 0 when prefersReducedMotion.current is true', async () => {
        // Clear module cache
        vi.resetModules()

        // Mock reduced motion preference as true
        mockMatchMedia(true)

        // Import module with reduced motion enabled
        const { DURATION_MOVE, DURATION_ARRIVE, DURATION_REMOVE } = await import(
          '$lib/animations/kanban-transitions'
        )

        // All durations should be exactly 0
        expect(DURATION_MOVE).toBe(0)
        expect(DURATION_ARRIVE).toBe(0)
        expect(DURATION_REMOVE).toBe(0)
      })
    })

    describe('AC6.1: Durations are non-zero when reduced motion is false', () => {
      it('sets durations to their defined values when prefersReducedMotion.current is false', async () => {
        // Clear module cache
        vi.resetModules()

        // Mock reduced motion preference as false
        mockMatchMedia(false)

        // Import module with reduced motion disabled
        const { DURATION_MOVE, DURATION_ARRIVE, DURATION_REMOVE } = await import(
          '$lib/animations/kanban-transitions'
        )

        // All durations should be non-zero
        expect(DURATION_MOVE).toBeGreaterThan(0)
        expect(DURATION_ARRIVE).toBeGreaterThan(0)
        expect(DURATION_REMOVE).toBeGreaterThan(0)
      })
    })
  })
})
