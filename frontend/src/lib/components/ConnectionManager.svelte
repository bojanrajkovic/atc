<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import { ConnectionManager } from '$lib/connection'

  // Base URL from current location. In dev, access via the backend port (e.g.
  // http://localhost:3000) — the backend's dev_proxy (atc-server/src/assets.rs)
  // forwards unknown routes to Vite at :5173. E2E tests run against Vite
  // directly (no backend), so ConnectionManager will fail to connect and cycle
  // through connecting/reconnecting — this is expected.
  const baseUrl = typeof window !== 'undefined' ? window.location.origin : 'http://localhost:3000'

  let manager: ConnectionManager | null = null

  onMount(() => {
    manager = new ConnectionManager(baseUrl)
    // connect() may reject on initial WS failure before the internal
    // reconnect loop takes over — swallow the rejection since the
    // manager handles reconnection internally.
    manager.connect().catch(() => {})
  })

  onDestroy(() => {
    manager?.destroy()
    manager = null
  })
</script>

<!-- Service component: no rendered DOM -->
