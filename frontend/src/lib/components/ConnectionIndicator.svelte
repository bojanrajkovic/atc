<script lang="ts">
  import * as Tooltip from '$lib/components/ui/tooltip'

  type IndicatorState = 'live' | 'stale' | 'connecting' | 'disconnected'

  let {
    state,
    detail,
    onreconnect = null,
  }: {
    state: IndicatorState
    detail: string
    /**
     * Optional callback fired when the user clicks the indicator. When set
     * (and only when `state === 'disconnected'`), the dot is rendered as a
     * `<button>` so keyboard / pointer users can re-arm the connect loop
     * after the manager has given up. When null, the indicator stays a
     * non-interactive `role="status"` span.
     */
    onreconnect?: (() => void) | null
  } = $props()

  const colorMap: Record<IndicatorState, string> = {
    live: 'var(--success)',
    stale: 'var(--running)',
    connecting: 'var(--queued)',
    disconnected: 'var(--failed)',
  }

  const color = $derived(colorMap[state])
  const interactive = $derived(state === 'disconnected' && onreconnect !== null)
</script>

<Tooltip.Provider>
  <Tooltip.Root>
    <Tooltip.Trigger>
      {#snippet child({ props })}
        {#if interactive}
          <button
            {...props}
            type="button"
            class="relative inline-flex h-3 w-3 shrink-0 cursor-pointer rounded-full"
            aria-label={detail}
            onclick={() => onreconnect?.()}
          >
            <span
              class="relative inline-flex h-3 w-3 rounded-full"
              style="background-color: {color};"
            ></span>
          </button>
        {:else}
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
        {/if}
      {/snippet}
    </Tooltip.Trigger>
    <Tooltip.Content>
      <p>{detail}</p>
    </Tooltip.Content>
  </Tooltip.Root>
</Tooltip.Provider>
