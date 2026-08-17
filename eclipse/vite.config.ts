import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, type ProxyOptions } from 'vite';

const backend = 'http://127.0.0.1:8000';
const backendProxy = (): ProxyOptions => ({
  target: backend,
  changeOrigin: true,
  configure(proxy) {
    proxy.on('proxyReq', (request) => {
      if (request.getHeader('origin')) request.setHeader('origin', backend);
    });
  }
});

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    host: '0.0.0.0',
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': backendProxy(),
      '/health': backendProxy(),
      '/images': backendProxy(),
      '/ws': {
        target: backend,
        changeOrigin: true,
        rewriteWsOrigin: true,
        ws: true
      }
    }
  }
});
