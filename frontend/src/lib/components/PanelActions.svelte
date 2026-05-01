<script lang="ts">
  export interface Props {
    htmlUrl: string
    onClose: () => void
  }

  let { htmlUrl, onClose }: Props = $props()
</script>

<div class="panel-actions">
  <a class="go-to-run" href={htmlUrl} target="_blank" rel="noopener noreferrer">
    Go to run
    <span class="external-icon" aria-hidden="true">↗</span>
  </a>
  <button class="close-button" type="button" onclick={onClose} aria-label="Close detail panel">
    ✕
  </button>
</div>

<style>
  .panel-actions {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    /* Padding aligned with PanelHeader so the actions row sits at the same
       top baseline as the status eyebrow when both share a flex row. */
    padding: 1rem 1.5rem 0 0;
    flex-shrink: 0;
  }
  .go-to-run {
    color: var(--accent);
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    font-size: 0.875rem;
    white-space: nowrap;
  }
  .go-to-run:hover {
    text-decoration: underline;
  }
  .close-button {
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0.25rem 0.5rem;
    color: var(--text-dim);
    line-height: 1;
    border-radius: 0.25rem;
  }
  .close-button:hover {
    color: var(--text);
    background: var(--surface-raised);
  }
</style>
