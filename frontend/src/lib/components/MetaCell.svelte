<script lang="ts">
  export interface Props {
    label: string
    value: string | undefined | null
    placeholder?: string // defaults to '—'
  }

  let { label, value, placeholder = '—' }: Props = $props()
  const display = $derived(
    value === undefined || value === null || value === '' ? placeholder : value
  )
</script>

<div class="meta-cell">
  <span class="label">{label}</span>
  <span class="value" class:placeholder={!value}>{display}</span>
</div>

<style>
  .meta-cell {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }
  .label {
    font-size: 0.75rem;
    color: var(--text-dim);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .value {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .value.placeholder {
    color: var(--text-dim);
  }
</style>
