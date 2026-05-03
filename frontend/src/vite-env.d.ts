/// <reference types="svelte" />
/// <reference types="vite/client" />

declare global {
  interface Window {
    __stores?: {
      runStore?: typeof import('$lib/stores/runs.svelte')['runStore']
      connectionStore?: typeof import('$lib/stores/connection.svelte')['connectionStore']
      runnerStore?: typeof import('$lib/stores/runners.svelte')['runnerStore']
      uiStore?: typeof import('$lib/stores/ui.svelte')['uiStore']
      paletteStore?: typeof import('$lib/stores/palette.svelte')['paletteStore']
      poolKey?: typeof import('$lib/filters/pool')['poolKey']
    }
    /** EventDispatcher singleton exposed for E2E harness (DEV only). */
    eventDispatcher?: typeof import('$lib/dispatcher')['eventDispatcher']
    /** Spy accumulator: element IDs passed to scrollIntoView, populated by e2e test harness. */
    __scrolledIds?: string[]
  }
}

export {}
