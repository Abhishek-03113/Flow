import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

// Tauri expects a fixed dev port and relative asset URLs in the bundle.
export default defineConfig({
  plugins: [react()],
  base: './',
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: 'es2022', sourcemap: false },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.test.{ts,tsx}'],
    css: false,
  },
});
