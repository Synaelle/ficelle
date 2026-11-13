// SPDX-License-Identifier: AGPL-3.0-or-later

import js from '@eslint/js'
import svelte from 'eslint-plugin-svelte'
import globals from 'globals'
import ts from 'typescript-eslint'

export default ts.config(
  {
    ignores: [
      '**/dist/',
      '**/dist-web/',
      '**/target/',
      '**/src-tauri/gen/',
      '**/src/generated/',
      // local folders kept out of git, whatever their names
      '.*/',
    ],
  },
  js.configs.recommended,
  ...ts.configs.strict,
  ...svelte.configs.recommended,
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        __APP_VERSION__: 'readonly',
        __APP_BUILD_DATE__: 'readonly',
        __APP_PROTOCOL__: 'readonly',
        __APP_RELEASE__: 'readonly',
        __APP_PREVIEW__: 'readonly',
        __APP_DEV_TOOLS__: 'readonly',
        __APP_PORTABLE__: 'readonly',
        __APP_PLATFORM__: 'readonly',
      },
    },
    rules: {
      // the tauri dialog plugin makes these async, a check like
      // `if (confirm(...))` then always passes
      'no-restricted-globals': [
        'error',
        ...['confirm', 'prompt', 'alert'].map((name) => ({
          name,
          message: 'Use overlay.ask or overlay.askText from lib/overlay.svelte.ts.',
        })),
      ],
    },
  },
  {
    // build tools and configs run in node, the app's code in a webview
    files: ['tools/**', '**/*.config.{js,ts}', 'apps/client/vite/**', 'packages/brand/**'],
    languageOptions: { globals: globals.node },
  },
  {
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: ts.parser },
    },
  },
)
