<script lang="ts">
	import { sourceStore } from '$lib/stores/sources.svelte';
	import { api } from '$lib/services/api';
	import type { SourceHealthEntry } from '$lib/services/api';
	import { onMount } from 'svelte';
	import { formatTimestamp } from '$lib/services/event-display';
	import { clockStore } from '$lib/stores/clock.svelte';

	let healthEntries = $state<SourceHealthEntry[]>([]);
	let healthLoading = $state(false);

	onMount(() => {
		sourceStore.refresh();
		loadHealth();
	});

	async function loadHealth() {
		healthLoading = true;
		try {
			healthEntries = await api.getSourcesHealth();
		} catch {
			/* silent */
		} finally {
			healthLoading = false;
		}
	}

	function statusColor(status: string): string {
		switch (status) {
			case 'healthy':
				return 'bg-success/20 text-success';
			case 'degraded':
				return 'bg-warning/20 text-warning';
			case 'down':
				return 'bg-alert/20 text-alert';
			default:
				return 'bg-bg-surface text-text-muted';
		}
	}

	function statusDot(status: string): string {
		switch (status) {
			case 'healthy':
				return 'bg-success';
			case 'degraded':
				return 'bg-warning';
			case 'down':
				return 'bg-alert';
			default:
				return 'bg-text-muted';
		}
	}
</script>

<div class="mx-auto max-w-4xl space-y-8 p-6">
	<h2 class="text-2xl font-bold text-text-primary">Settings</h2>

	<!-- Source Health -->
	<section>
		<div class="mb-3 flex items-center justify-between">
			<h3 class="text-lg font-semibold text-text-primary">Source Health</h3>
			<button
				onclick={loadHealth}
				class="rounded px-2 py-1 text-xs text-text-muted transition-colors hover:bg-bg-surface hover:text-text-primary"
				title="Refresh source health data"
			>
				Refresh
			</button>
		</div>
		{#if healthLoading}
			<p class="text-sm text-text-muted">Loading health data...</p>
		{:else if healthEntries.length === 0}
			<p class="text-sm text-text-muted">No source health data available.</p>
		{:else}
			<div class="space-y-1.5">
				{#each healthEntries as entry (entry.source_id)}
					<div
						class="flex items-center justify-between rounded-md border border-border-default bg-bg-card px-4 py-2.5"
					>
						<div class="flex items-center gap-3">
							<span class="h-2 w-2 rounded-full {statusDot(entry.status)}" title={entry.status}></span>
							<div>
								<span class="font-mono text-sm font-medium text-text-primary">{entry.source_id}</span>
								<span class="ml-2 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase {statusColor(entry.status)}">
									{entry.status}
								</span>
							</div>
						</div>
						<div class="flex items-center gap-4 text-xs text-text-muted">
							{#if entry.consecutive_failures > 0}
								<span class="text-alert" title="Consecutive failures">
									{entry.consecutive_failures} fail{entry.consecutive_failures !== 1 ? 's' : ''}
								</span>
							{/if}
							<span title="Events in last 24 hours">
								{entry.total_events_24h.toLocaleString()} events/24h
							</span>
							{#if entry.last_success}
								<span title="Last successful fetch">
									Last OK: {formatTimestamp(entry.last_success, clockStore.now)}
								</span>
							{/if}
						</div>
					</div>
					{#if entry.last_error}
						<div class="ml-5 rounded border border-alert/20 bg-alert/5 px-3 py-1.5 font-mono text-[10px] text-alert/80">
							{entry.last_error}
						</div>
					{/if}
				{/each}
			</div>
		{/if}
	</section>

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
				{#each sourceStore.sources as source (source.id)}
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
