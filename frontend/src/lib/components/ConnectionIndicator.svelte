<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip'

  type IndicatorState = 'live' | 'stale' | 'connecting' | 'disconnected'

  let { state, detail }: { state: IndicatorState; detail: string } = $props()

  const colorMap: Record<IndicatorState, string> = {
    live: 'var(--success)',
    stale: 'var(--running)',
    connecting: 'var(--queued)',
    disconnected: 'var(--failed)',
  }

  const color = $derived(colorMap[state])
</script>

<Tooltip.Provider>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        <span
          {...props}
          class="relative inline-flex h-3 w-3 shrink-0"
          role="status"
          aria-label={detail}
        >
          {#if state === 'connecting'}
            <span
              class="absolute inline-flex h-full w-full animate-ping rounded-full opacity-75"
              style="background-color: {color};"
            ></span>
          {/if}
          <span
            class="relative inline-flex h-3 w-3 rounded-full"
            style="background-color: {color}; {state === 'live'
              ? `box-shadow: 0 0 6px 2px ${color};`
              : ''}"
          ></span>
        </span>
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>
      <p>{detail}</p>
    </Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>
