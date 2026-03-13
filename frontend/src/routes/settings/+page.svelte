<script lang="ts">
	import { sourceStore } from '$lib/stores/sources.svelte';
	import { onMount } from 'svelte';

	onMount(() => {
		sourceStore.refresh();
	});
</script>

<div class="mx-auto max-w-4xl space-y-8 p-6">
	<h2 class="text-2xl font-bold text-text-primary">Settings</h2>

	<!-- Source Configuration -->
	<section>
		<h3 class="mb-3 text-lg font-semibold text-text-primary">Data Sources</h3>
		{#if sourceStore.loading}
			<p class="text-sm text-text-muted">Loading sources...</p>
		{:else if sourceStore.sources.length === 0}
			<p class="text-sm text-text-muted">
				No sources registered. Sources will appear once the backend has them configured.
			</p>
		{:else}
			<div class="space-y-2">
				{#each sourceStore.sources as source}
					<div
						class="flex items-center justify-between rounded-md border border-border-default bg-bg-card px-4 py-3"
					>
						<div>
							<span class="text-sm font-medium text-text-primary">{source.name}</span>
							<span class="ml-2 text-xs text-text-muted">{source.id}</span>
							{#if source.health}
								<span
									class="ml-2 rounded px-1.5 py-0.5 text-xs
									{source.health.status === 'healthy'
										? 'bg-success/20 text-success'
										: source.health.status === 'degraded'
											? 'bg-warning/20 text-warning'
											: source.health.status === 'down'
												? 'bg-alert/20 text-alert'
												: 'bg-bg-surface text-text-muted'}"
								>
									{source.health.status}
								</span>
							{/if}
						</div>
						<button
							class="rounded-md px-3 py-1 text-sm {source.config?.enabled
								? 'bg-success/20 text-success'
								: 'bg-bg-surface text-text-muted'}"
							onclick={() => sourceStore.toggleSource(source.id)}
							title={source.config?.enabled ? 'Click to disable this source' : 'Click to enable this source'}
						>
							{source.config?.enabled ? 'Enabled' : 'Disabled'}
						</button>
					</div>
				{/each}
			</div>
		{/if}
	</section>
</div>
