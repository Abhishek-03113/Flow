import { defineConfig } from '@playwright/test';
import fs from 'node:fs';

// Exploratory QA round (npx playwright test -c playwright.qa.config.ts). Failing cases are open findings.
const root = process.env.PLAYWRIGHT_BROWSERS_PATH ?? '/opt/pw-browsers';
const dir = fs.existsSync(root) ? fs.readdirSync(root).find((d) => /^chromium-\d+$/.test(d)) : undefined;
const executablePath = dir ? `${root}/${dir}/chrome-linux/chrome` : undefined;

export default defineConfig({
  testDir: './e2e-qa',
  testIgnore: ['live/**'],
  outputDir: './test-results-qa',
  fullyParallel: false,
  workers: 1,
  reporter: [['list'], ['json', { outputFile: process.env.QA_JSON ?? 'test-results-qa/qa-results.json' }]],
  use: {
    baseURL: 'http://127.0.0.1:4173',
    viewport: { width: 1100, height: 720 },
    launchOptions: { executablePath, args: ['--no-sandbox'] },
  },
  webServer: {
    command: 'npm run preview -- --host 127.0.0.1',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
