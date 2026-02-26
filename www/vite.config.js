import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    fs: {
      // Allow serving files from the pkg directory during dev
      allow: ['..']
    }
  },
  build: {
    target: 'esnext',
    assetsInlineLimit: 0, // Don't inline WASM files
  },
  // Vite automatically handles WASM files when imported!
  // No need for custom copy logic
});
