<!--
  Version-mismatch banner (issue #47).

  Locked-after-impeccable design:
  - Full-width strip mounted in AppShell between TopBar and <main>.
  - Surface tinted via OKLCH color-mix with --queued at 6% — NO 3px side
    stripe (that pattern is in the design system's absolute-bans list).
  - Glyph (↺) in --text-dim, not in the tone color. Brand says status
    colors are the only high-chroma elements; the tone lives only on the
    emphasized {N}s number and the countdown bar.
  - Entrance: ease-out-expo, 220ms, slide-up-and-fade — no overshoot
    (impeccable: no bounce/elastic).
  - prefers-reduced-motion: bar element hidden entirely; the numeric "{N}s"
    carries state.

  --queued is reused here as a calm-informational tone, NOT as a workflow-
  status indicator. The banner is a transient deploy-detected notice; the
  tone's "blue == informational" carries over without implying any
  workflow is in Queued state.
-->

<script lang="ts">
  import { onMount } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import { connectionStore } from '$lib/stores/connection.svelte'

  const COUNTDOWN_TOTAL_MS = 30_000

  // Wall-clock state driven by a 1Hz interval. The bar visual itself uses a
  // CSS @keyframes animation (compositor-driven, 60+ Hz, zero JS overhead);
  // the interval exists only to update the numeric "{N}s" text and to trip
  // the auto-reload at zero.
  let now = $state(Date.now())
  let tickHandle: ReturnType<typeof setInterval> | null = null

  // prefers-reduced-motion is watched live — the user can flip the OS-level
  // setting during the countdown and the bar disappears immediately.
  let reduceMotion = $state(false)

  $effect(() => {
    const mql = window.matchMedia('(prefers-reduced-motion: reduce)')
    reduceMotion = mql.matches
    const onChange = (e: MediaQueryListEvent) => {
      reduceMotion = e.matches
    }
    mql.addEventListener('change', onChange)
    return () => mql.removeEventListener('change', onChange)
  })

  onMount(() => {
    tickHandle = setInterval(() => {
      now = Date.now()
    }, 1_000)
    return () => {
      if (tickHandle !== null) clearInterval(tickHandle)
    }
  })

  // Banner visibility — driven entirely by store state.
  const visible = $derived(
    connectionStore.serverVersionMismatch !== null && connectionStore.serverReloadAt !== null
  )

  // Remaining millis derives from the 1Hz `now`; the bar's CSS animation runs
  // independently from this value (it's keyed by `serverReloadAt` below).
  const remainingMs = $derived(
    connectionStore.serverReloadAt !== null ? Math.max(0, connectionStore.serverReloadAt - now) : 0
  )
  const remainingSeconds = $derived(Math.ceil(remainingMs / 1000))

  // Auto-reload at zero. Guarded by `visible` so we only fire once the banner
  // is actually showing — defensive against any future state shape where
  // serverReloadAt is set but the banner isn't visible.
  $effect(() => {
    if (visible && remainingMs <= 0) {
      connectionStore.refreshNow()
    }
  })
</script>

{#if visible}
  <aside
    role="status"
    aria-live="polite"
    aria-atomic="true"
    class="version-mismatch-banner"
    style="
      background-color: color-mix(in oklch, var(--surface) 94%, var(--queued) 6%);
      border-bottom: 1px solid var(--border);
      color: var(--text);
    "
  >
    <span class="icon" aria-hidden="true">↺</span>
    <span class="copy">
      A new build is available — refreshing in <span class="secs">{remainingSeconds}</span>s
    </span>
    {#if !reduceMotion}
      <!--
        {#key serverReloadAt} forces Svelte to destroy + recreate the bar
        when a new mismatch arrives mid-countdown (so the CSS @keyframes
        animation restarts cleanly from 100%). Without the key, the deadline
        would change but the running animation would keep draining from its
        prior frame.
      -->
      {#key connectionStore.serverReloadAt}
        <div class="countdown-bar" data-countdown-bar aria-hidden="true">
          <div class="countdown-bar-fill" style="animation-duration: {COUNTDOWN_TOTAL_MS}ms"></div>
        </div>
      {/key}
    {/if}
    <Button size="sm" onclick={() => connectionStore.refreshNow()}>Refresh now</Button>
  </aside>
{/if}

<style>
  .version-mismatch-banner {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 16px;
    font-size: 13px;
    line-height: 1.5;
    flex-shrink: 0;
    animation: banner-in 220ms cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes banner-in {
    from {
      opacity: 0;
      transform: translateY(-8px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .version-mismatch-banner {
      animation: none;
    }
  }

  .icon {
    flex-shrink: 0;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    font-size: 13px;
    color: var(--text-dim);
    font-weight: 600;
  }

  .copy {
    flex: 1;
    color: var(--text);
  }

  .secs {
    color: var(--queued);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .countdown-bar {
    flex-shrink: 0;
    width: 120px;
    height: 3px;
    background: var(--surface-raised);
    border-radius: 999px;
    overflow: hidden;
  }

  /*
    transform: scaleX runs on the compositor (no layout/paint cost per frame
    like width: %), so the bar drains at the display refresh rate without
    burning main-thread work. transform-origin: left makes it drain from
    right-to-left visually like a width animation would.
  */
  .countdown-bar-fill {
    height: 100%;
    background: var(--queued);
    border-radius: 999px;
    transform-origin: left center;
    animation-name: countdown-drain;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
    /* animation-duration set inline */
  }

  @keyframes countdown-drain {
    from {
      transform: scaleX(1);
    }
    to {
      transform: scaleX(0);
    }
  }
</style>
