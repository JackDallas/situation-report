<script lang="ts">
	import { onMount, onDestroy } from 'svelte';

	interface PipelineMetrics {
		events_ingested: number;
		events_correlated: number;
		events_enriched: number;
		events_published: number;
		events_filtered: number;
		incidents_created: number;
	}

	let metrics = $state<PipelineMetrics | null>(null);
	let prevMetrics = $state<PipelineMetrics | null>(null);
	let rates = $state<Record<string, number>>({});
	let timer: ReturnType<typeof setInterval>;

	async function fetchMetrics() {
		try {
			const res = await fetch('/api/pipeline/metrics');
			if (res.ok) {
				const data = await res.json();
				if (metrics) {
					// Calculate rates (per second, since we poll every 5s)
					const dt = 5;
					rates = {
						ingested:
							Math.round(((data.events_ingested - metrics.events_ingested) / dt) * 10) / 10,
						published:
							Math.round(((data.events_published - metrics.events_published) / dt) * 10) / 10,
						filtered:
							Math.round(((data.events_filtered - metrics.events_filtered) / dt) * 10) / 10,
						enriched:
							Math.round(((data.events_enriched - metrics.events_enriched) / dt) * 10) / 10
					};
				}
				prevMetrics = metrics;
				metrics = data;
			}
		} catch {
			/* ignore */
		}
	}

	onMount(() => {
		fetchMetrics();
		timer = setInterval(fetchMetrics, 5000);
	});

	onDestroy(() => clearInterval(timer));

	function fmt(n: number): string {
		if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + 'M';
		if (n >= 1_000) return (n / 1_000).toFixed(1) + 'K';
		return n.toString();
	}

	const stages = [
		{ id: 'ingest', label: 'Ingest', key: 'events_ingested', color: '#3b82f6' },
		{ id: 'correlate', label: 'Correlate', key: 'events_correlated', color: '#f97316' },
		{ id: 'enrich', label: 'Enrich', key: 'events_enriched', color: '#a855f7' },
		{ id: 'publish', label: 'Publish', key: 'events_published', color: '#10b981' }
	] as const;
</script>

<div class="flex h-full flex-col gap-3 overflow-y-auto p-3">
	<div class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
		Pipeline Flow
	</div>

	{#if metrics}
		<!-- Pipeline stages -->
		<div class="flex items-center gap-1">
			{#each stages as stage, i}
				{@const count = metrics[stage.key as keyof PipelineMetrics] ?? 0}
				{@const rate = rates[stage.id] ?? 0}
				<div
					class="flex-1 rounded border border-border-default bg-bg-secondary p-2 text-center"
					title={stage.id === 'ingest' ? 'Total events received from all sources' : stage.id === 'correlate' ? 'Events processed by correlation rules' : stage.id === 'enrich' ? 'Events enriched by AI (Haiku/Ollama)' : 'Events published to SSE for frontend display'}
				>
					<div
						class="text-[9px] font-semibold uppercase tracking-wider"
						style="color: {stage.color};"
					>
						{stage.label}
					</div>
					<div class="mt-1 text-lg font-bold text-text-primary">{fmt(count)}</div>
					{#if rate > 0}
						<div class="text-[10px] text-text-muted">{rate}/s</div>
					{/if}
				</div>
				{#if i < stages.length - 1}
					<svg
						class="h-4 w-4 flex-shrink-0 text-text-muted"
						viewBox="0 0 24 24"
						role="img"
						aria-label="flows to next stage"
						fill="none"
						stroke="currentColor"
					>
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M9 5l7 7-7 7"
						/>
					</svg>
				{/if}
			{/each}
		</div>

		<!-- Stats grid -->
		<div class="grid grid-cols-2 gap-2">
			<div class="rounded border border-border-default bg-bg-secondary p-2">
				<div class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">
					Incidents
				</div>
				<div class="mt-1 text-xl font-bold text-red-400" title="Total correlated incidents detected by pipeline rules">{fmt(metrics.incidents_created)}</div>
			</div>
			<div class="rounded border border-border-default bg-bg-secondary p-2">
				<div class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">
					Filtered
				</div>
				<div class="mt-1 text-xl font-bold text-yellow-400" title="Events below importance threshold or absorbed into summaries">{fmt(metrics.events_filtered)}</div>
			</div>
		</div>

		<!-- Flow ratio bar -->
		{@const total = metrics.events_ingested || 1}
		{@const publishedPct = Math.round((metrics.events_published / total) * 100)}
		{@const filteredPct = Math.round((metrics.events_filtered / total) * 100)}
		<div class="rounded border border-border-default bg-bg-secondary p-2">
			<div class="mb-1.5 text-[9px] font-semibold uppercase tracking-wider text-text-muted">
				Event Flow Ratio
			</div>
			<div class="flex h-3 overflow-hidden rounded-full bg-bg-surface">
				<div
					class="bg-emerald-500/60"
					style="width: {publishedPct}%;"
					title="Published: {publishedPct}%"
				></div>
				<div
					class="bg-yellow-500/60"
					style="width: {filteredPct}%;"
					title="Filtered: {filteredPct}%"
				></div>
				<div
					class="flex-1 bg-border-default"
					title="Other: {100 - publishedPct - filteredPct}%"
				></div>
			</div>
			<div class="mt-1 flex justify-between text-[9px] text-text-muted">
				<span class="text-emerald-400">Published {publishedPct}%</span>
				<span class="text-yellow-400">Filtered {filteredPct}%</span>
			</div>
		</div>
	{:else}
		<div class="flex items-center justify-center py-8 text-text-muted">Loading metrics...</div>
	{/if}
</div>
