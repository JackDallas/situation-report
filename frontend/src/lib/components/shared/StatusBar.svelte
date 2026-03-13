<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import type { SourceInfo } from '$lib/types/sources';

	let { sources = [], eventsPerMinute = 0 }: { sources: SourceInfo[]; eventsPerMinute: number } =
		$props();

	const healthyCount = $derived(sources.filter((s) => s.health?.status === 'healthy').length);
	const totalSources = $derived(sources.length);

	const statusColors = {
		connected: 'bg-success',
		reconnecting: 'bg-warning animate-pulse',
		disconnected: 'bg-alert'
	} as const;

	const statusLabels = {
		connected: 'Live',
		reconnecting: 'Reconnecting...',
		disconnected: 'Disconnected'
	} as const;

	const tempo = $derived.by(() => {
		if (eventsPerMinute > 20) return { label: 'HIGH', color: 'text-alert', dot: 'bg-alert' };
		if (eventsPerMinute > 5)
			return { label: 'ELEVATED', color: 'text-warning', dot: 'bg-warning' };
		return { label: 'NORMAL', color: 'text-success', dot: 'bg-success' };
	});

	const utcTime = $derived.by(() => {
		const d = new Date(clockStore.now);
		return d.toISOString().slice(11, 16) + 'Z';
	});

	let budget = $state<{
		spent_today_usd: number;
		daily_budget_usd: number;
		budget_exhausted: boolean;
		degraded: boolean;
	} | null>(null);

	const budgetPct = $derived(
		budget ? Math.min(100, (budget.spent_today_usd / budget.daily_budget_usd) * 100) : 0
	);
	const budgetColor = $derived(
		budget?.budget_exhausted ? 'bg-alert' : budget?.degraded ? 'bg-warning' : 'bg-accent'
	);

	async function pollBudget() {
		try {
			const resp = await fetch('/api/intel/budget');
			if (resp.ok) budget = await resp.json();
		} catch {
			/* silent */
		}
	}

	$effect(() => {
		pollBudget();
		const interval = setInterval(pollBudget, 30_000);
		return () => clearInterval(interval);
	});
</script>

<div
	class="flex h-7 shrink-0 items-center justify-between border-t border-border-default bg-bg-primary px-4 text-[11px]"
>
	<!-- Left: connection status -->
	<div class="flex items-center gap-4">
		<div class="flex items-center gap-1.5" title="SSE connection: {statusLabels[eventStore.connectionStatus]}">
			<span class="relative flex h-2 w-2">
				{#if eventStore.connectionStatus === 'connected'}
					<span
						class="absolute inline-flex h-full w-full animate-ping rounded-full bg-success opacity-75"
					></span>
				{/if}
				<span
					class="relative inline-flex h-2 w-2 rounded-full {statusColors[
						eventStore.connectionStatus
					]}"
				></span>
			</span>
			<span class="text-text-secondary">{statusLabels[eventStore.connectionStatus]}</span>
		</div>

		{#if totalSources > 0}
			<span class="text-text-muted" title="{healthyCount} of {totalSources} data sources reporting healthy">{healthyCount}/{totalSources} sources</span>
		{/if}

		<span class="font-mono text-text-muted/70" title="Current time in UTC">{utcTime}</span>
	</div>

	<!-- Right: rate + tempo -->
	<div class="flex items-center gap-4">
		{#if eventsPerMinute > 0}
			<span class="text-text-muted" title="Events per minute across all sources (5-minute average)">~{eventsPerMinute}/min</span>
		{/if}

		{#if eventStore.incidentCount > 0}
			<span class="animate-pulse font-bold text-red-400" title="Active correlated incidents requiring attention">
				{eventStore.incidentCount} incident{eventStore.incidentCount !== 1 ? 's' : ''}
			</span>
		{/if}

		{#if budget}
			<div class="flex items-center gap-1.5" title="AI budget: ${budget.spent_today_usd.toFixed(2)} / ${budget.daily_budget_usd.toFixed(0)}/day">
				<span class="text-text-muted">AI</span>
				<div class="h-1 w-12 rounded-full bg-bg-surface">
					<div class="h-1 rounded-full {budgetColor}" style="width: {budgetPct}%"></div>
				</div>
				<span class="text-text-muted">${budget.spent_today_usd.toFixed(2)}</span>
			</div>
		{/if}

		<div class="flex items-center gap-1.5" title="Event tempo: {tempo.label} ({eventsPerMinute > 20 ? 'high event rate, >20/min' : eventsPerMinute > 5 ? 'elevated event rate, >5/min' : 'normal event rate'})">
			<span class="h-1.5 w-1.5 rounded-full {tempo.dot}"></span>
			<span class="font-semibold tracking-wider {tempo.color}">{tempo.label}</span>
		</div>
	</div>
</div>
