<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { AUTH_PROBE_TIMEOUT_MS } from '$lib/connection'
  import { connectionStore } from '$lib/stores/connection.svelte'

  // One-shot: fires on the first `connected` transition, not on every
  // reconnect. mode=none deployments never mount /v1/auth/me, so the probe
  // 404s once and connectionStore.identity stays null — no chrome, no spam.
  let fetched = false

  $effect(() => {
    if (connectionStore.status !== 'connected' || fetched) return
    fetched = true
    fetch('/v1/auth/me', { signal: AbortSignal.timeout(AUTH_PROBE_TIMEOUT_MS) })
      .then((res) => (res.ok ? res.json() : null))
      .then((identity) => {
        // A 401 elsewhere may have cleared connectionStore.identity (and
        // moved to unauthenticated) while this fetch was still in flight —
        // don't resurrect stale identity data on top of that.
        if (identity && connectionStore.status !== 'unauthenticated') {
          connectionStore.identity = identity
        }
      })
      .catch(() => {
        // Network hiccup (or a timeout) on a one-shot probe isn't worth
        // retry machinery — the chrome simply stays absent until the next
        // full page load.
      })
  })

  async function logout(): Promise<void> {
    await fetch('/v1/auth/github/logout', { method: 'POST' }).catch(() => {})
    window.location.href = '/'
  }
</script>

{#if connectionStore.identity !== null}
  <div class="flex items-center gap-2 text-xs" style="color: var(--text-dim);">
    <span>{connectionStore.identity.login}</span>
    <Button size="xs" variant="ghost" onclick={logout}>Log out</Button>
  </div>
{/if}
