<script lang="ts">
	import '../app.css';
	import { onMount, onDestroy } from 'svelte';
	import { connectSSE, disconnectSSE } from '$lib/services/sse';
	import { digestStore } from '$lib/stores/digest.svelte';
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
