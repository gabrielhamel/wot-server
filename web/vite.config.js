import { defineConfig } from 'vite';

export default defineConfig({
  build: { target: 'chrome100' },
  esbuild: { jsx: 'automatic' },
  server: {
    host: '127.0.0.1',
    proxy: {
      '/api': 'http://127.0.0.1:8080',
    },
  },
});
