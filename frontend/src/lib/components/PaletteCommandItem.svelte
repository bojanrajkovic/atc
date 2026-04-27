<script lang="ts">
  import * as Command from '$lib/components/ui/command'

  export interface Props {
    label: string
    icon?: string
    shortcut?: string[]
    onSelect: () => void
  }

  let { label, icon, shortcut, onSelect }: Props = $props()
</script>

<Command.Item value={label} {onSelect}>
  {#if icon}<span class="icon">{icon}</span>{/if}
  <span class="label">{label}</span>
  {#if shortcut}
    <span class="shortcut">
      {#each shortcut as key, i (i)}
        <kbd>{key}</kbd>
      {/each}
    </span>
  {/if}
</Command.Item>

<style>
  .shortcut {
    margin-left: auto;
    display: inline-flex;
    gap: 0.25rem;
  }

  kbd {
    background: var(--kbd-bg);
    border: 1px solid var(--kbd-border);
    border-radius: 0.25rem;
    padding: 0.125rem 0.375rem;
    font-family: monospace;
    font-size: 0.85em;
  }
</style>
