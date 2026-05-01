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
  <!--
    data-slot="command-shortcut" is always present (even when empty) so the
    upstream CheckIcon in command-item.svelte is hidden via its
    `:has([data-slot=command-shortcut])` selector. This keeps every command
    row's right edge consistent — empty rows collapse the wrapper to zero
    width (no `margin-left:auto` push) instead of reserving 16px for the
    invisible CheckIcon.
  -->
  <span class="shortcut" data-slot="command-shortcut">
    {#if shortcut}
      {#each shortcut as key, i (i)}
        <kbd>{key}</kbd>
      {/each}
    {/if}
  </span>
</Command.Item>

<style>
  .shortcut {
    margin-left: auto;
    display: inline-flex;
    gap: 0.25rem;
  }

  /* Collapse the empty wrapper so non-shortcut rows align flush-left
     instead of reserving the right-edge slot. */
  .shortcut:empty {
    margin-left: 0;
    display: none;
  }

  kbd {
    background: var(--kbd-bg);
    border: 1px solid var(--kbd-border);
    border-radius: 0.25rem;
    padding: 0.125rem 0.375rem;
    font-family: monospace;
    font-size: 0.85em;
    /* Equal-width keys so multi-key shortcuts (⌘+D, ⌘+\) align vertically
       across rows regardless of the second key's character width. */
    min-width: 1.6em;
    text-align: center;
    box-sizing: border-box;
  }
</style>
