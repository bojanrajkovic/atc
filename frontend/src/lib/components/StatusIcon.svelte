<script lang="ts">
  import type { StatusKey } from '$lib/format/status-key'

  let { value }: { value: StatusKey } = $props()

  const SYMBOLS = {
    Queued: { symbol: '\u25D0', label: 'Queued' }, // ◐
    InProgress: { symbol: '\u25B6', label: 'In Progress' }, // ▶
    Success: { symbol: '\u2713', label: 'Success' }, // ✓
    Failure: { symbol: '\u2717', label: 'Failure' }, // ✗
    Cancelled: { symbol: '\u2298', label: 'Cancelled' }, // ⊘
    TimedOut: { symbol: '\u23F1', label: 'Timed Out' }, // ⏱
    ActionRequired: { symbol: '\u26A0', label: 'Action Required' }, // ⚠
    StartupFailure: { symbol: '\u26A1', label: 'Startup Failure' }, // ⚡
    Stale: { symbol: '\u25CB', label: 'Stale' }, // ○
    Neutral: { symbol: '\u25CB', label: 'Neutral' }, // ○
    Skipped: { symbol: '\u25CB', label: 'Skipped' }, // ○
  } as const satisfies Record<StatusKey, { symbol: string; label: string }>

  const info = $derived(SYMBOLS[value])
</script>

<span class="status-icon" style="color: var(--status-color);">
  <span aria-hidden="true">{info.symbol}</span>
  <span class="sr-only">{info.label}</span>
</span>
