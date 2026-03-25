import tsParser from '@typescript-eslint/parser'
import svelte from 'eslint-plugin-svelte'
import svelteParser from 'svelte-eslint-parser'

export default [
  {
    ignores: ['dist/', 'node_modules/', '**/*.ts', '**/*.js', '!eslint.config.mjs'],
  },
  ...svelte.configs['flat/recommended'],
  {
    files: ['**/*.svelte'],
    languageOptions: {
      parser: svelteParser,
      parserOptions: {
        parser: tsParser,
      },
    },
    rules: {
      // Strict rules beyond recommended
      'svelte/button-has-type': 'error',
      'svelte/no-at-debug-tags': 'error',
      'svelte/no-reactive-reassign': 'error',
      'svelte/no-target-blank': 'error',
      'svelte/no-useless-mustaches': 'error',
      'svelte/require-each-key': 'error',
      'svelte/require-event-dispatcher-types': 'error',
      'svelte/valid-each-key': 'error',
    },
  },
]
