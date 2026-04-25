import './app.css'
import { mount } from 'svelte'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import App from './App.svelte'

// Expose stores for E2E testing (harmless no-op in production)
if (import.meta.env.DEV) {
  // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
  ;(window as any).__stores = { runStore, connectionStore, runnerStore }
}

const appElement = document.getElementById('app')
if (!appElement) {
  throw new Error('Could not find root element with id="app"')
}

const app = mount(App, {
  target: appElement,
})

export default app
