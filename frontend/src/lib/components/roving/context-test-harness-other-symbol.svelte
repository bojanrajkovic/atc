<script lang="ts">
  /**
   * Test harness that sets a context under an arbitrary symbol (NOT ROVING_CONTEXT_KEY)
   * and then renders the GET harness as a child. Used to verify that ROVING_CONTEXT_KEY
   * isolation prevents unrelated context values from leaking into getRovingContext().
   */
  import { setContext } from 'svelte'
  import ContextTestHarnessGet from './context-test-harness-get.svelte'
  import type { RovingFocusContext } from './context'

  type GetResult = { ok: true; value: RovingFocusContext } | { ok: false; error: Error }

  let { onResult }: { onResult: (r: GetResult) => void } = $props()

  // Register something under a different symbol — not ROVING_CONTEXT_KEY
  const otherSymbol = Symbol('other')
  setContext(otherSymbol, { impostor: true })
</script>

<ContextTestHarnessGet {onResult} />
