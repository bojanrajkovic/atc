<script lang="ts">
  import type { StatusKey } from '$lib/format/status-key'
  import StatusIcon from './StatusIcon.svelte'

  let {
    displayTitle,
    statusValue,
    durationText,
  }: {
    displayTitle: string
    statusValue: StatusKey
    durationText: string
  } = $props()
</script>

<div class="run-card-header flex items-center gap-2">
  <StatusIcon value={statusValue} />
  <span class="run-card-name truncate flex-1 font-medium">{displayTitle}</span>
  <span class="run-card-duration tabular-nums text-xs text-muted-foreground">
    {durationText}
  </span>
</div>
