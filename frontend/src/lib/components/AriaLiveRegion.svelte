<script lang="ts">
  import { liveRegion } from '$lib/aria/live-region.svelte'
</script>

<div
  role="status"
  aria-live="polite"
  aria-atomic="true"
  aria-busy={liveRegion.busy ? 'true' : 'false'}
  aria-label="Workflow run updates"
  class="sr-only"
>
  {liveRegion.message}
</div>
