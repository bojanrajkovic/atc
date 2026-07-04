<script lang="ts">
  import { Button } from '$lib/components/ui/button'
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
