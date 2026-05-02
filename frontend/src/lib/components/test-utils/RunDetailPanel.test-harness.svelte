<script lang="ts">
  import RunDetailPanel from '../RunDetailPanel.svelte'
  import RovingFocusProvider from '../roving/RovingFocusProvider.svelte'

  /**
   * Synthetic card descriptor used to populate the fake kanban DOM inside the
   * harness. Each entry renders as:
   *   <article class="run-card" data-run-id="{runId}">
   *     <button class="run-card-activate" type="button">{label}</button>
   *   </article>
   *
   * Tests that simulate a trigger-card still mounted add the card's id to this
   * array. Tests that simulate eviction omit the trigger card's id entirely
   * (so querySelector for it returns null — the bug path).
   */
  interface SyntheticCard {
    runId: bigint
    label: string
  }

  interface Props {
    cards?: SyntheticCard[]
  }
  let { cards = [] }: Props = $props()
</script>

<!--
  Wrap the panel in a real RovingFocusProvider so RunDetailPanel's
  getRovingContext() call resolves. The synthetic kanban DOM sits inside
  the provider so restoreFocusToInitial() can querySelector within it.
-->
<RovingFocusProvider>
  <div id="synthetic-kanban">
    {#each cards as card (card.runId)}
      <article class="run-card" data-run-id={card.runId}>
        <button class="run-card-activate" type="button">{card.label}</button>
      </article>
    {/each}
  </div>
  <RunDetailPanel />
</RovingFocusProvider>
