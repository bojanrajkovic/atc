<script lang="ts">
  /**
   * Combined test harness: mounts SET (parent) with GET (child) in a single
   * Svelte component tree so that Svelte's context propagation works correctly.
   * The onResult callback receives the result of calling getRovingContext()
   * inside the child's init scope.
   */
  import { setRovingContext, type RovingFocusContext } from './context'
  import ContextTestHarnessGet from './context-test-harness-get.svelte'

  type GetResult = { ok: true; value: RovingFocusContext } | { ok: false; error: Error }

  let { ctx, onResult }: { ctx: RovingFocusContext; onResult: (r: GetResult) => void } = $props()

  setRovingContext(ctx)
</script>

<ContextTestHarnessGet {onResult} />
