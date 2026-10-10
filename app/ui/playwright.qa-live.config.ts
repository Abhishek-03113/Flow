import { defineConfig } from '@playwright/test';
import live from './playwright.live.config';

// Exploratory QA probes against real engines (`flow-harness`). Failing cases are open findings.
export default defineConfig({
  ...live,
  testDir: './e2e-qa/live',
  outputDir: './test-results-qa-live',
});
