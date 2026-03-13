<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';

	const statusColors = {
		connected: 'bg-success',
		reconnecting: 'bg-warning animate-pulse',
		disconnected: 'bg-alert',
	} as const;

	const statusLabels = {
		connected: 'Live',
		reconnecting: 'Reconnecting...',
		disconnected: 'Disconnected',
	} as const;
</script>

<div class="flex items-center gap-2 text-sm" title="Server-Sent Events connection: {statusLabels[eventStore.connectionStatus]}">
	<span class="relative flex h-2.5 w-2.5">
		{#if eventStore.connectionStatus === 'connected'}
			<span class="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-75"></span>
		{/if}
		<span class="relative inline-flex h-2.5 w-2.5 rounded-full {statusColors[eventStore.connectionStatus]}"></span>
	</span>
	<span class="text-text-secondary">{statusLabels[eventStore.connectionStatus]}</span>
</div>
