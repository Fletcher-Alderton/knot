import { defineConfig } from 'vite';

const devHost = process.env.TAURI_DEV_HOST;

export default defineConfig({
  server: {
    host: devHost || undefined,
    port: 1420,
    strictPort: true,
    hmr: devHost ? { host: devHost, protocol: 'ws', port: 1421 } : undefined,
  },
  test: {
    environment: 'jsdom',
    globals: true,
  },
});
