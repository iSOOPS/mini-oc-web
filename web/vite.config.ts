import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  server: {
    host: true, // listen on 0.0.0.0 so PowerShell IPv4 (127.0.0.1) can reach Vite
    proxy: {
      '/api': 'http://127.0.0.1:8100',
    },
  },
  build: {
    outDir: 'dist',
  },
})
