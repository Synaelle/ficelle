// SPDX-License-Identifier: AGPL-3.0-or-later

import js from '@eslint/js'
import svelte from 'eslint-plugin-svelte'
import globals from 'globals'
import ts from 'typescript-eslint'

export default ts.config(
  {
    ignores: [
      '**/dist/',
      '**/target/',
      '**/src-tauri/gen/',
      '**/src/generated/',
      '.atelier/',
      '.dev-data/',
    ],
  },
  js.configs.recommended,
  ...ts.configs.strict,
  ...svelte.configs.recommended,
  {
    languageOptions: {
      globals: {
        ...globals.browser,
        ...globals.node,
        __APP_VERSION__: 'readonly',
        __APP_BUILD_DATE__: 'readonly',
        __APP_PROTOCOL__: 'readonly',
        __APP_RELEASE__: 'readonly',
        __APP_DEV_REPORTS__: 'readonly',
        __APP_PORTABLE__: 'readonly',
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
    files: ['**/*.svelte', '**/*.svelte.ts'],
    languageOptions: {
      parserOptions: { parser: ts.parser },
    },
  },
)
