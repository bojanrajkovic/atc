import path from 'node:path'

import { svelte } from '@sveltejs/vite-plugin-svelte'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  resolve: {
    alias: {
      $lib: path.resolve('./src/lib'),
    },
  },
  server: {
    hmr: {
      // The HMR error overlay covers the page and intercepts pointer events.
      // `@bgotink/playwright-coverage` fetches `.svelte?svelte&type=style&
      // lang.css.map` source maps as a fallback when no inline sourcemap is
      // present (see `node_modules/@bgotink/playwright-coverage/lib/data.js`'s
      // `getSourceMap` last-resort branch); Vite routes those `.map` requests
      // through `@tailwindcss/vite:generate:serve`, which parses the JSON
      // map body as CSS and throws `Invalid declaration: <ts-identifier>`.
      // The overlay then breaks any subsequent Playwright `click` because
      // it sits on top of the click target. Disabling the overlay lets the
      // app stay interactive — the Tailwind errors still print to the dev
      // server's stdout for debugging, and they don't propagate into the
      // running app's runtime (the `.css.map` fetches fail server-side but
      // the actual `<style>` block is already inlined and live).
      overlay: false,
    },
  },
})
