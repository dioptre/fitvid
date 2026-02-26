import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    fs: {
      // Allow serving files from the pkg directory
      allow: ['..']
    }
  },
  build: {
    target: 'esnext'
  }
});
