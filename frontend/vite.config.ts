import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	server: {
		proxy: {
			'/api/sse': {
				target: 'http://localhost:3001',
				changeOrigin: true,
				// SSE: disable response compression so events stream immediately
				headers: { 'Accept-Encoding': 'identity' },
				configure: (proxy, _options) => {
					// @ts-expect-error ProxyServer inherits EventEmitter.on at runtime
					proxy.on('proxyRes', (proxyRes: { headers: Record<string, string> }) => {
						proxyRes.headers['cache-control'] = 'no-cache';
						proxyRes.headers['x-accel-buffering'] = 'no';
						delete proxyRes.headers['content-encoding'];
					});
				},
			},
			'/api': {
				target: 'http://localhost:3001',
				changeOrigin: true,
			},
		},
	},
});
