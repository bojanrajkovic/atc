<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { connectionStore } from '$lib/stores/connection.svelte'
  import Logo from './Logo.svelte'

  function computeLoginHref(): string {
    const returnTo = window.location.pathname + window.location.search
    return `/v1/auth/github/login?return_to=${encodeURIComponent(returnTo)}`
  }

  // Kept reasonably fresh via popstate (back/forward is the main way the URL
  // changes while this screen stays mounted) so a non-standard interaction
  // that skips the click handler below — copy link address, middle-click —
  // still carries a correct return_to rather than silently defaulting to /.
  let loginHref = $state(computeLoginHref())

  $effect(() => {
    const onPopstate = () => {
      loginHref = computeLoginHref()
    }
    window.addEventListener('popstate', onPopstate)
    return () => window.removeEventListener('popstate', onPopstate)
  })

  // Recomputed at click time too: a same-tick history.replaceState (e.g.
  // App.svelte's popstate handler stripping a stale ?run= once its run isn't
  // in the now-cleared runStore) doesn't fire its own popstate event, so the
  // href above can still lag a half-step behind for a normal left-click.
  function login(event: MouseEvent): void {
    event.preventDefault()
    window.location.href = computeLoginHref()
  }

  // Popup-first silent re-auth for a staleness signal (not auth_required,
  // which always needs the explicit click above — there's no prior GitHub
  // session to silently refresh). window.open must be called synchronously
  // in whatever context noticed the staleness to have any chance at
  // transient user activation; called from an async continuation it would
  // always be blocked. Most of the time there's no activation at all (an
  // unattended dashboard reconnecting), and that's fine — the null-fallback
  // below is what keeps it self-healing without a gesture.
  let popupInFlight = false

  $effect(() => {
    if (connectionStore.authReason !== 'stale_authorization' || popupInFlight) return
    popupInFlight = true

    const popup = window.open(
      '/v1/auth/github/login?popup=1',
      'atc-auth',
      'popup,width=640,height=760'
    )

    if (popup === null) {
      popupInFlight = false
      window.location.href = computeLoginHref()
      return
    }

    const channel = new BroadcastChannel('atc-auth')
    let pollTimer: ReturnType<typeof setInterval> | null = null

    const cleanup = () => {
      if (pollTimer !== null) clearInterval(pollTimer)
      channel.close()
      popupInFlight = false
    }

    channel.onmessage = (event) => {
      if (event.data !== 'session-refreshed') return
      cleanup()
      connectionStore.retry()
    }

    // The popup self-closes after posting session-refreshed (see auth.rs's
    // POPUP_CALLBACK_HTML) — this only fires if the user closes it manually
    // instead, abandoning the login. Degrade to the already-visible login
    // screen rather than waiting forever.
    pollTimer = setInterval(() => {
      if (popup.closed) cleanup()
    }, 500)

    return cleanup
  })
</script>

<main
  class="flex flex-col items-center justify-center h-dvh gap-6 px-4"
  style="background-color: var(--bg);"
>
  <Logo />
  <p class="text-sm text-center max-w-sm" style="color: var(--text-dim);">
    Sign in with GitHub to see the workflow runs you're authorized to monitor.
  </p>
  <Button href={loginHref} onclick={login} size="lg">Sign in with GitHub</Button>
</main>
