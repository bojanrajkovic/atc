<script lang="ts">
  import * as Command from '$lib/components/ui/command'

  export interface Props {
    heading: string
    children?: import('svelte').Snippet
  }

  let { heading, children }: Props = $props()
</script>

<Command.Group {heading}>
  {@render children?.()}
</Command.Group>
