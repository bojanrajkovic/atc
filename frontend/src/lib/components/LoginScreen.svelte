<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import Logo from './Logo.svelte'

  const returnTo = window.location.pathname + window.location.search
  const loginHref = `/v1/auth/github/login?return_to=${encodeURIComponent(returnTo)}`
</script>

<main
  class="flex flex-col items-center justify-center h-dvh gap-6 px-4"
  style="background-color: var(--bg);"
>
  <Logo />
  <p class="text-sm text-center max-w-sm" style="color: var(--text-dim);">
    Sign in with GitHub to see the workflow runs you're authorized to monitor.
  </p>
  <Button href={loginHref} size="lg">Sign in with GitHub</Button>
</main>
