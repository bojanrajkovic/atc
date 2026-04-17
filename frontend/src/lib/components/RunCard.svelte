<!--
  SCOPE CONTRACT — Sub-Phase 3 (Kanban Board skeleton)

  This component is intentionally minimal. The following are FORBIDDEN
  in this phase and will be added in Sub-Phase 4:

  - Imports: StatusIcon, ProgressBar, JobMeta, JobHeader, RunnerLabel
  - CSS: @keyframes rules (pulsating halo)
  - JS: setInterval or recurring $effect (duration ticker)
  - Content: progress bar, meta (repo/branch), runner label, accent bar

  This comment is temporary. Remove it as Sub-Phase 4's first task.
-->

<script lang="ts">
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  let { run }: { run: WorkflowRun } = $props()

  const STATUS_MAP: Record<string, { color: string; glyph: string; label: string }> = {
    Queued: { color: 'var(--queued)', glyph: '\u25CB', label: 'Queued' },
    InProgress: {
      color: 'var(--running)',
      glyph: '\u25B6',
      label: 'In Progress',
    },
    Completed: { color: 'var(--text-dim)', glyph: '\u25CF', label: 'Completed' },
  }

  const statusInfo = $derived(STATUS_MAP[run.status] || STATUS_MAP['Queued']!)
</script>

<div data-run-id={run.id} class="flex items-center gap-3">
  <span class="inline-flex items-center gap-1" style="color: {statusInfo.color};">
    <span>{statusInfo.glyph}</span>
    <span class="sr-only">{statusInfo.label}</span>
  </span>
  <span class="text-sm">{run.displayTitle}</span>
</div>
