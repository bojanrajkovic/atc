import { cubicOut } from 'svelte/easing'
import { prefersReducedMotion } from 'svelte/motion'
import { crossfade, fade, fly } from 'svelte/transition'

// Single reduced-motion check — when true, all durations collapse to 0.
// This controls crossfade, flip, and fallback transitions from one place.
const reduced = prefersReducedMotion.current

export const DURATION_MOVE = reduced ? 0 : 300
export const DURATION_ARRIVE = reduced ? 0 : 250
export const DURATION_REMOVE = reduced ? 0 : 200
export const FLY_SETTLE_Y = 20

const [send, receive] = crossfade({
  duration: DURATION_MOVE,
  easing: cubicOut,
  fallback(node, _params, intro) {
    if (intro) {
      // New card arriving — fly in from below
      return fly(node, {
        y: FLY_SETTLE_Y,
        duration: DURATION_ARRIVE,
        easing: cubicOut,
      })
    }
    // Card being removed — fade out
    return fade(node, {
      duration: DURATION_REMOVE,
      easing: cubicOut,
    })
  },
})

export { receive, send }
