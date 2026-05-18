<script lang="ts">
  /*
   * Config-reload-error admin alert banner (issue #203).
   *
   * Surfaces the backend's `WireFrame::ConfigReloadError` reason string when
   * a runner-pool hot-reload fails. The backend keeps serving last-known-good
   * capacities, so this banner is informational-but-actionable: it tells an
   * operator looking at the dashboard "your edit was rejected, fix and save
   * again."
   *
   * Locked-after-impeccable design (mirrors VersionMismatchBanner.svelte):
   * - Full-width strip mounted in AppShell adjacent to VersionMismatchBanner.
   * - Surface tinted via OKLCH color-mix with --failed at 6%. NO 3px side
   *   stripe (design-system absolute-bans list rejects that pattern).
   * - Glyph (`✗`, the impeccable Failed-state assignment) in --text-dim, not
   *   in the tone color. Status colors are the only high-chroma elements;
   *   the tone lives on the surface tint alone (this banner has no
   *   countdown bar or emphasized numeric to carry the tone).
   * - Entrance: ease-out-expo, 220ms, slide-up-and-fade. No overshoot.
   * - prefers-reduced-motion: entrance animation disabled.
   * - aria-live="polite" + aria-atomic="true" so a last-wins reason
   *   replacement triggers a full re-announcement, not partial output.
   *
   * The 60s wall-clock auto-dismiss lives on the store (markConfigReloadError
   * in connection.svelte.ts) — not as a component $effect — because the
   * timer is opaque (no visible countdown) and the store-side placement lets
   * unit tests drive auto-dismiss without mounting Svelte.
   *
   * Comment lives inside <script> rather than as a top-level HTML comment so
   * the Tailwind v4 Vite plugin (which tokenizes the whole .svelte file for
   * class-name extraction) does not choke on stray apostrophes.
   */
  import { Button } from '$lib/components/ui/button'
  import { connectionStore } from '$lib/stores/connection.svelte'

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

  const visible = $derived(connectionStore.configReloadError !== null)
</script>

{#if visible}
  <aside
    role="status"
    aria-live="polite"
    aria-atomic="true"
    aria-label="Config reload failed on the server"
    class="config-reload-error-banner"
    style="
      background-color: color-mix(in oklch, var(--surface) 94%, var(--failed) 6%);
      border-bottom: 1px solid var(--border);
      color: var(--text);
    "
  >
    <span
      class="icon"
      data-banner-glyph
      data-banner-motion={reduceMotion ? undefined : ''}
      aria-hidden="true">✗</span
    >
    <span class="copy">
      Config reload failed on server: <span class="reason">{connectionStore.configReloadError}</span
      >
    </span>
    <Button
      size="icon-sm"
      variant="ghost"
      aria-label="Dismiss"
      onclick={() => connectionStore.dismissConfigReloadError()}
    >
      <span aria-hidden="true">×</span>
    </Button>
  </aside>
{/if}

<style>
  .config-reload-error-banner {
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
    .config-reload-error-banner {
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

  .reason {
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
</style>
