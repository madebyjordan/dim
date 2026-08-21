import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, type ProxyOptions } from 'vite';

const backend = 'http://127.0.0.1:8000';
const backendProxy = (): ProxyOptions => ({
  target: backend,
  changeOrigin: true,
  configure(proxy) {
    proxy.on('proxyReq', (request, incoming) => {
      if (request.getHeader('origin')) request.setHeader('origin', backend);
      // Preserve client origin across the loopback development proxy. The backend accepts these
      // private headers only from loopback and uses them to distinguish Safari from an AirPlay
      // receiver without relying on device-specific user agents.
      const clientIp = incoming.socket.remoteAddress?.replace(/^::ffff:/, '');
      if (clientIp) request.setHeader('x-eclipse-proxy-client-ip', clientIp);
      if (incoming.headers.host)
        request.setHeader('x-eclipse-proxy-origin-host', incoming.headers.host);
    });
  }
});

export default defineConfig({
  plugins: [sveltekit()],
  resolve: {
    conditions: ['browser']
  },
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
