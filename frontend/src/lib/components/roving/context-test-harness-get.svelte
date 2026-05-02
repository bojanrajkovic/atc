<script lang="ts">
  import { getRovingContext, type RovingFocusContext } from './context'

  type GetResult = { ok: true; value: RovingFocusContext } | { ok: false; error: Error }

  let { onResult }: { onResult: (r: GetResult) => void } = $props()

  try {
    const value = getRovingContext()
    // eslint-disable-next-line svelte/no-unused-svelte-ignore
    // svelte-ignore state_referenced_locally -- init-time read is intentional in this test harness
    onResult({ ok: true, value })
  } catch (e) {
    // eslint-disable-next-line svelte/no-unused-svelte-ignore
    // svelte-ignore state_referenced_locally -- init-time read is intentional in this test harness
    onResult({ ok: false, error: e instanceof Error ? e : new Error(String(e)) })
  }
</script>
