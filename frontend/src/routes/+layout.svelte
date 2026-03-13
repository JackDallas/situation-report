<script lang="ts">
	import '../app.css';
	import { onMount, onDestroy } from 'svelte';
	import { connectSSE, disconnectSSE } from '$lib/services/sse';
	import { digestStore } from '$lib/stores/digest.svelte';
	import { themeStore } from '$lib/stores/theme.svelte';
	import DigestOverlay from '$lib/components/shared/DigestOverlay.svelte';
	import type { Snippet } from 'svelte';

	let { children }: { children: Snippet } = $props();

	function handleBeforeUnload() {
		digestStore.saveTimestamp();
	}

	onMount(() => {
		connectSSE();
		window.addEventListener('beforeunload', handleBeforeUnload);
	});

	onDestroy(() => {
		disconnectSSE();
		window.removeEventListener('beforeunload', handleBeforeUnload);
	});
</script>

<div class="flex h-screen flex-col overflow-hidden bg-bg-primary">
	<!-- Header -->
	<header
		class="flex h-10 shrink-0 items-center justify-between border-b border-border-default px-4"
	>
		<div class="flex items-center gap-3">
			<a href="/" class="text-sm font-bold tracking-tight text-text-primary"> SITUATION REPORT</a>
			<span class="rounded bg-alert/20 px-1.5 py-0.5 text-[10px] font-medium text-alert" title="Real-time event streaming is active"
				>LIVE</span
			>
		</div>
		<div class="flex items-center gap-1">
			<button
				onclick={() => themeStore.toggle()}
				class="rounded-md px-2 py-1 text-text-secondary hover:bg-bg-surface hover:text-text-primary"
				title={themeStore.current === 'dark' ? 'Switch to light theme' : 'Switch to dark theme'}
				aria-label="Toggle theme"
			>
				{#if themeStore.current === 'dark'}
					<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
					</svg>
				{:else}
					<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
					</svg>
				{/if}
			</button>
			<a
				href="/graph"
				class="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-text-secondary hover:bg-bg-surface hover:text-text-primary"
				title="View entity relationship graph"
			>
				<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
					<circle cx="5" cy="6" r="2" stroke-width="2" />
					<circle cx="19" cy="6" r="2" stroke-width="2" />
					<circle cx="12" cy="18" r="2" stroke-width="2" />
					<path stroke-width="2" d="M7 7l3.5 9M17 7l-3.5 9M7 6h10" />
				</svg>
				Graph
			</a>
			<a
				href="/settings"
				class="rounded-md px-2 py-1 text-xs text-text-secondary hover:bg-bg-surface hover:text-text-primary"
			>
				Settings
			</a>
		</div>
	</header>

	<!-- Main content -->
	<main class="min-h-0 flex-1 overflow-auto">
		{@render children()}
	</main>

	<DigestOverlay />
</div>
