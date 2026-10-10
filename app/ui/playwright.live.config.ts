import { defineConfig } from '@playwright/test';
import fs from 'node:fs';

// The real UI against live virtual engines (`flow-harness up`), see e2e-live/.
// Uses the preinstalled Chromium (never `playwright install`).
const root = process.env.PLAYWRIGHT_BROWSERS_PATH ?? '/opt/pw-browsers';
const dir = fs.existsSync(root) ? fs.readdirSync(root).find((d) => /^chromium-\d+$/.test(d)) : undefined;
const executablePath = dir ? `${root}/${dir}/chrome-linux/chrome` : undefined;

export default defineConfig({
  testDir: './e2e-live',
  outputDir: './test-results-live',
  globalSetup: './e2e-live/global-setup.ts',
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  reporter: process.env.CI
    ? [['list'], ['html', { outputFolder: 'playwright-report-live', open: 'never' }]]
    : [['list']],
  use: {
    baseURL: 'http://127.0.0.1:4174',
    viewport: { width: 1100, height: 720 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
    launchOptions: { executablePath, args: ['--no-sandbox'] },
  },
  webServer: {
    command: 'npm run build && npx vite preview --port 4174 --strictPort --host 127.0.0.1',
    url: 'http://127.0.0.1:4174',
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
