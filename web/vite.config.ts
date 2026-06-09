import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  base: './',
  // 不清空 outDir:保留提交的 web/dist/.gitkeep(rust-embed 需目录存在;dist 内容 gitignore)。
  build: { emptyOutDir: false },
  server: {
    proxy: {
      '/api/ws': {
        target: 'ws://127.0.0.1:8080',
        ws: true,
        changeOrigin: true,
      },
      '/api': {
        target: 'http://127.0.0.1:8080',
        changeOrigin: true,
      },
    },
  },
})
