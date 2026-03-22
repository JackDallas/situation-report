<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import { formatAbsoluteTime, formatTimestamp, formatFullTimestamp } from '$lib/services/event-display';

	const report = $derived(eventStore.latestAnalysis);

	function tempoColor(tempo: string): string {
		switch (tempo) {
			case 'HIGH': return 'bg-alert/20 text-alert';
			case 'ELEVATED': return 'bg-warning/20 text-warning';
			default: return 'bg-success/20 text-success';
		}
	}

	function escalationColor(assessment: string): string {
		if (assessment.startsWith('CRITICAL')) return 'text-alert';
		if (assessment.startsWith('ELEVATED')) return 'text-warning';
		if (assessment.startsWith('WATCH')) return 'text-yellow-400';
		return 'text-success';
	}

	import { ENTITY_TYPE_ICONS } from '$lib/config/colors';
	import { api } from '$lib/services/api';
	import type { BudgetStatus } from '$lib/services/api';

	let budgetData = $state<BudgetStatus | null>(null);

	async function loadBudget() {
		try {
			budgetData = await api.getBudget();
		} catch { /* silent */ }
	}

	$effect(() => {
		loadBudget();
		const interval = setInterval(loadBudget, 60_000);
		return () => clearInterval(interval);
	});
</script>

<div class="flex h-full flex-col">
	<!-- Header -->
	<div class="border-b border-border-default px-4 py-2">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<svg class="h-3.5 w-3.5 text-text-muted" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden="true">
					<path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
				</svg>
				<span class="text-xs font-semibold uppercase tracking-wider text-text-secondary"
					>Situation Report</span
				>
				{#if report}
					<span class="rounded-full px-2 py-0.5 text-[10px] font-bold {tempoColor(report.tempo)}" title="Event tempo: {report.tempo} — determines analysis frequency">
						{report.tempo}
					</span>
				{/if}
			</div>
			{#if report}
				<span class="text-[10px] text-text-muted" title={formatFullTimestamp(report.timestamp)}>
						{formatAbsoluteTime(report.timestamp, clockStore.now)} <span class="opacity-60">{formatTimestamp(report.timestamp, clockStore.now)}</span>
					</span>
			{/if}
		</div>
	</div>

	<!-- Content -->
	<div class="flex-1 overflow-auto">
		{#if !report}
			<div class="flex h-full items-center justify-center text-text-muted">
				<div class="text-center">
					<svg class="mx-auto mb-2 h-8 w-8 text-text-muted/50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M9 12h6m-6 4h6m2 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
					</svg>
					<p class="text-sm">No analysis yet</p>
					<p class="mt-1 text-xs">Intelligence briefs appear based on event tempo</p>
				</div>
			</div>
		{:else}
			<div class="space-y-4 p-4">
				<!-- Escalation Assessment -->
				<div class="rounded-lg border border-border-default bg-bg-card p-3">
					<h4 class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Escalation</h4>
					<p class="mt-1 text-[13px] font-medium {escalationColor(report.escalation_assessment)}" title="Escalation assessment based on cross-source intelligence analysis">
						{report.escalation_assessment}
					</p>
				</div>

				<!-- Narrative -->
				<div>
					<h4 class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Analysis</h4>
					<div class="mt-1.5 space-y-2 text-[12px] leading-relaxed text-text-secondary">
						{#each report.narrative.split('\n\n') as paragraph}
							{#if paragraph.trim()}
								<p>{paragraph.trim()}</p>
							{/if}
						{/each}
					</div>
				</div>

				<!-- Topic Clusters -->
				{#if report.topic_clusters.length > 0}
					<div>
						<h4 class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Topic Clusters</h4>
						<div class="mt-1.5 space-y-2">
							{#each report.topic_clusters as cluster}
								<div class="rounded border border-border-default bg-bg-card p-2">
									<div class="flex items-center justify-between">
										<span class="text-[11px] font-medium text-text-primary">{cluster.label}</span>
										<span class="text-[10px] text-text-muted">{cluster.event_count} events</span>
									</div>
									<div class="mt-1 flex flex-wrap gap-1">
										{#each cluster.topics as topic}
											<span class="rounded bg-accent/10 px-1.5 py-0.5 text-[9px] text-accent">{topic}</span>
										{/each}
									</div>
									{#if cluster.regions.length > 0}
										<div class="mt-1 text-[10px] text-text-muted">
											Regions: {cluster.regions.join(', ')}
										</div>
									{/if}
								</div>
							{/each}
						</div>
					</div>
				{/if}

				<!-- Key Entities -->
				{#if report.key_entities.length > 0}
					<div>
						<h4 class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Key Entities</h4>
						<div class="mt-1.5 space-y-1.5">
							{#each report.key_entities as entity}
								<div class="flex items-start gap-2">
									<span class="mt-0.5 rounded px-1.5 py-0.5 text-[9px] font-medium {ENTITY_TYPE_ICONS[entity.entity_type] ?? 'bg-bg-surface text-text-muted'}">
										{entity.entity_type}
									</span>
									<div class="min-w-0 flex-1">
										<span class="text-[11px] font-medium text-text-primary">{entity.entity_name}</span>
										<span class="ml-1 text-[10px] text-text-muted">({entity.source_count} sources)</span>
										<p class="mt-0.5 text-[10px] leading-relaxed text-text-muted">{entity.context}</p>
									</div>
								</div>
							{/each}
						</div>
					</div>
				{/if}

				<!-- Suggested Merges -->
				{#if report.suggested_merges.length > 0}
					<div>
						<h4 class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Suggested Links</h4>
						<div class="mt-1.5 space-y-1.5">
							{#each report.suggested_merges as merge}
								<div class="rounded border border-border-default bg-bg-card p-2">
									<div class="flex items-center gap-1">
										<span class="rounded bg-accent/10 px-1 py-0.5 text-[9px] font-medium text-accent">
											{(merge.confidence * 100).toFixed(0)}%
										</span>
										{#if merge.suggested_title}
											<span class="text-[11px] font-medium text-text-primary">{merge.suggested_title}</span>
										{/if}
									</div>
									<p class="mt-0.5 text-[10px] text-text-muted">{merge.reason}</p>
								</div>
							{/each}
						</div>
					</div>
				{/if}

				<!-- Budget -->
				{#if budgetData}
					<div class="rounded-lg border border-border-default bg-bg-card p-3">
						<h4 class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">AI Budget</h4>
						<div class="mt-1.5 flex items-center gap-3">
							<div class="flex-1">
								<div class="h-1.5 w-full rounded-full bg-bg-surface">
									<div
										class="h-1.5 rounded-full transition-all {budgetData.budget_exhausted ? 'bg-alert' : budgetData.degraded ? 'bg-warning' : 'bg-success'}"
										style="width: {Math.min(100, (budgetData.spent_today_usd / budgetData.daily_budget_usd) * 100)}%"
									></div>
								</div>
							</div>
							<span class="text-[10px] text-text-muted">
								${budgetData.spent_today_usd.toFixed(2)} / ${budgetData.daily_budget_usd.toFixed(0)}
							</span>
						</div>
						{#if budgetData.degraded}
							<p class="mt-1 text-[10px] text-warning">Sonnet analysis paused (budget conservation)</p>
						{/if}
						{#if budgetData.budget_exhausted}
							<p class="mt-1 text-[10px] text-alert">Budget exhausted — enrichment paused until tomorrow</p>
						{/if}
					</div>
				{/if}

				<!-- Model info -->
				<div class="text-[10px] text-text-muted/50" title="AI model and token usage for this analysis">
					{report.model} | {report.tokens_used.toLocaleString()} tokens
				</div>
			</div>
		{/if}
	</div>
</div>
