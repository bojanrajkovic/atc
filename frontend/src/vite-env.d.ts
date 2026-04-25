/// <reference types="svelte" />
/// <reference types="vite/client" />

declare global {
  interface Window {
    __stores?: {
      runStore?: typeof import('$lib/stores/runs.svelte')['runStore']
      connectionStore?: typeof import('$lib/stores/connection.svelte')['connectionStore']
      runnerStore?: typeof import('$lib/stores/runners.svelte')['runnerStore']
    }
  }
}

export {}
