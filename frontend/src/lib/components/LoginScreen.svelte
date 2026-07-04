<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import Logo from './Logo.svelte'

  // Computed at click time, not at mount: the URL can change while this
  // screen stays mounted (e.g. a stale ?run= deep link stripped by
  // App.svelte's popstate handler once the run isn't in the now-cleared
  // runStore), and return_to should reflect wherever the user actually is
  // when they click, not a snapshot from whenever the login screen first
  // rendered.
  function login(event: MouseEvent): void {
    event.preventDefault()
    const returnTo = window.location.pathname + window.location.search
    window.location.href = `/v1/auth/github/login?return_to=${encodeURIComponent(returnTo)}`
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
  <Button href="/v1/auth/github/login" onclick={login} size="lg">Sign in with GitHub</Button>
</main>
