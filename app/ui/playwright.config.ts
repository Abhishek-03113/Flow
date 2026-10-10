import { defineConfig } from '@playwright/test';
import fs from 'node:fs';

// Use the preinstalled Chromium (never `playwright install`).
const root = process.env.PLAYWRIGHT_BROWSERS_PATH ?? '/opt/pw-browsers';
const dir = fs.existsSync(root) ? fs.readdirSync(root).find((d) => /^chromium-\d+$/.test(d)) : undefined;
const executablePath = dir ? `${root}/${dir}/chrome-linux/chrome` : undefined;

export default defineConfig({
  testDir: './e2e',
  outputDir: './test-results',
  fullyParallel: false,
  workers: 1,
  reporter: [['list']],
  use: {
    baseURL: 'http://127.0.0.1:4173',
    viewport: { width: 1100, height: 720 },
    launchOptions: {
      executablePath,
      args: ['--remote-debugging-port=9222', '--no-sandbox'],
    },
  },
  webServer: {
    command: 'npm run build && npm run preview -- --host 127.0.0.1',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: true,
    timeout: 120_000,
  },
});
