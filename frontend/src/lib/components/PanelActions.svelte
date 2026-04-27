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
    gap: 1rem;
    align-items: center;
    padding: 0 1.5rem 1rem;
  }
  .go-to-run {
    color: var(--accent);
    text-decoration: none;
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
  .go-to-run:hover {
    text-decoration: underline;
  }
  .close-button {
    background: transparent;
    border: none;
    cursor: pointer;
    padding: 0.5rem;
    margin-left: auto;
    color: var(--text-dim);
  }
  .close-button:hover {
    color: var(--text);
  }
</style>
