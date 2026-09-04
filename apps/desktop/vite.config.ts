import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: 'jsdom',
    globals: true,
  },
});
