<script lang="ts">
  import { untrack } from 'svelte'
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

  // Popup-first for both auth reasons — window.open must be called
  // synchronously in whatever context noticed the reason to have any chance
  // at transient user activation; called from an async continuation it
  // would always be blocked. A fresh cold visit (typed URL, bookmark) can
  // still carry activation into window.open in some browser/automation
  // contexts (verified empirically — do not assume it's always blocked), so
  // treat "popup succeeds" as a real, reachable case for auth_required too,
  // not just for an existing SPA session getting revoked mid-use.
  let popupInFlight = $state(false)

  // Cancels whatever popup-flow cleanup is currently registered, if any —
  // set by the effect below while a popup is open, cleared (null) once it
  // isn't. The manual click always wins over a competing background popup:
  // the backend's OAuth flow cookie is a single slot per browser, not scoped
  // per popup/tab, so leaving both alive would let one clobber the other's
  // cookie and fail with a state mismatch. Canceling first — rather than
  // just disabling the button until the popup resolves — means the button
  // never gets stuck: a popup that silently never completes (blocked by
  // browser chrome our code can't see, or simply ignored) would otherwise
  // leave the user looking at a disabled button with no way forward.
  let cancelPopup: (() => void) | null = null

  // Recomputed at click time too: a same-tick history.replaceState (e.g.
  // App.svelte's popstate handler stripping a stale ?run= once its run isn't
  // in the now-cleared runStore) doesn't fire its own popstate event, so the
  // href above can still lag a half-step behind for a normal left-click.
  function login(event: MouseEvent): void {
    event.preventDefault()
    cancelPopup?.()
    window.location.href = computeLoginHref()
  }

  $effect(() => {
    // untrack the popupInFlight read: it's a guard against re-entrancy, not
    // a reactive trigger — this effect also writes it, and reading it as a
    // tracked dependency of an effect that writes it is exactly the
    // read-your-own-write cycle Svelte's effect_update_depth_exceeded guards
    // against. connectionStore.authReason is the only real trigger.
    const reason = connectionStore.authReason
    if (reason === null || untrack(() => popupInFlight)) return
    popupInFlight = true

    let popup: Window | null
    try {
      popup = window.open('/v1/auth/github/login?popup=1', 'atc-auth', 'popup,width=640,height=760')
    } catch {
      // Blocked entirely (e.g. a sandboxed iframe without allow-popups) —
      // treat the same as window.open returning null.
      popup = null
    }

    if (popup === null) {
      popupInFlight = false
      // stale_authorization: the tab already has a working session and this
      // is often an unattended reconnect with nobody watching — auto-redirect
      // so it self-heals. auth_required: could be a genuinely fresh visitor
      // who's never seen this app's GitHub consent screen; auto-navigating
      // them into it with zero warning is the back-button-trap UX this
      // screen exists to avoid (see docs/architecture/frontend-app.md). Leave
      // the button as the only path there.
      if (reason === 'stale_authorization') window.location.href = computeLoginHref()
      return
    }

    const channel = new BroadcastChannel('atc-auth')
    let pollTimer: ReturnType<typeof setInterval> | null = null
    let graceTimer: ReturnType<typeof setTimeout> | null = null

    const cleanup = () => {
      if (pollTimer !== null) clearInterval(pollTimer)
      if (graceTimer !== null) clearTimeout(graceTimer)
      channel.close()
      if (!popup.closed) popup.close()
      popupInFlight = false
      cancelPopup = null
    }
    cancelPopup = cleanup

    channel.onmessage = (event) => {
      if (event.data !== 'session-refreshed') return
      cleanup()
      connectionStore.retry()
    }

    // The popup self-closes right after posting session-refreshed (see
    // auth.rs's POPUP_CALLBACK_HTML), and BroadcastChannel delivery is
    // asynchronous — popup.closed can flip true a moment before the message
    // actually arrives. Stop polling immediately but give the channel a
    // grace window before tearing it down, so an already-sent message still
    // lands instead of being silently discarded by channel.close().
    pollTimer = setInterval(() => {
      if (!popup.closed) return
      if (pollTimer !== null) clearInterval(pollTimer)
      pollTimer = null
      graceTimer = setTimeout(cleanup, 1000)
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
