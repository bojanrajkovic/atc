<script lang="ts">
  import { getRovingContext, type RovingFocusContext } from './context'

  type GetResult = { ok: true; value: RovingFocusContext } | { ok: false; error: Error }

  let { onResult }: { onResult: (r: GetResult) => void } = $props()

  try {
    const value = getRovingContext()
    onResult({ ok: true, value })
  } catch (e) {
    onResult({ ok: false, error: e instanceof Error ? e : new Error(String(e)) })
  }
</script>
