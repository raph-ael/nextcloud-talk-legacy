import { resolve } from 'node:path'
import { defineConfig } from 'electron-vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  main: {
    resolve: { alias: { '@shared': resolve('src/shared') } },
  },
  preload: {
    resolve: { alias: { '@shared': resolve('src/shared') } },
  },
  renderer: {
    resolve: { alias: { '@shared': resolve('src/shared') } },
    plugins: [react()],
    build: {
      minify: true,
      // Never inline scripts as data: URLs (the AudioWorklet must load from 'self' under the CSP).
      assetsInlineLimit: (file: string) => (file.endsWith('.js') ? false : undefined),
    },
  },
})
