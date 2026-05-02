<script lang="ts">
  import { setRovingContext, type RovingFocusContext } from './context'
  import type { Snippet } from 'svelte'

  let {
    ctx,
    children,
  }: {
    ctx: RovingFocusContext
    children?: Snippet
  } = $props()

  setRovingContext(ctx)
</script>

{@render children?.()}
