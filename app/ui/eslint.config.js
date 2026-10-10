import js from '@eslint/js';
import jsxA11y from 'eslint-plugin-jsx-a11y';
import reactHooks from 'eslint-plugin-react-hooks';
import globals from 'globals';
import tseslint from 'typescript-eslint';

export default tseslint.config(
  {
    ignores: [
      'dist',
      'test-results',
      'test-results-live',
      'playwright-report',
      'playwright-report-live',
      'node_modules',
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  jsxA11y.flatConfigs.recommended,
  {
    files: ['**/*.{ts,tsx}'],
    languageOptions: { globals: { ...globals.browser } },
    plugins: { 'react-hooks': reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // v7's new rule flags the deliberate "reset local state when a prop/store value changes" effects
      // (ArrangementEditor, ShortcutsView, usePairingFlow); they are intentional, so keep them as-is.
      'react-hooks/set-state-in-effect': 'off',
      '@typescript-eslint/consistent-type-imports': ['error', { fixStyle: 'inline-type-imports' }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_', varsIgnorePattern: '^_' }],
    },
  },
  { files: ['*.config.{js,ts}', 'e2e/**', 'e2e-live/**'], languageOptions: { globals: { ...globals.node } } },
);
