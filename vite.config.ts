import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'

// Settings recommended for Tauri: fixed port, keep Rust errors visible, don't watch src-tauri
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  build: {
    target: 'safari15',
  },
  test: {
    include: ['src/**/*.test.ts'],
  },
})
