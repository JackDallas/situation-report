<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import { api } from '$lib/services/api';
	import type { BudgetStatus } from '$lib/services/api';
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

	let budget = $state<BudgetStatus | null>(null);

	type GpuStateValue = 'on' | 'starting' | 'off' | 'stopping';
	let gpuState = $state<GpuStateValue>('on');
	let gpuToggling = $state(false);

	const gpuTransitioning = $derived(gpuState === 'starting' || gpuState === 'stopping');

	const budgetPct = $derived(
		budget ? Math.min(100, (budget.spent_today_usd / budget.daily_budget_usd) * 100) : 0
	);
	const budgetColor = $derived(
		budget?.budget_exhausted ? 'bg-alert' : budget?.degraded ? 'bg-warning' : 'bg-accent'
	);

	async function pollBudget() {
		try {
			budget = await api.getBudget();
		} catch {
			/* silent */
		}
	}

	async function pollGpuStatus() {
		try {
			const data = await api.getPipelineMetrics();
			if (data.gpu_state) {
				gpuState = data.gpu_state as GpuStateValue;
			} else {
				// Fallback for old backend without gpu_state
				gpuState = data.gpu_paused ? 'off' : 'on';
			}
		} catch {
			/* silent */
		}
	}

	async function toggleGpu() {
		if (gpuTransitioning) return;
		gpuToggling = true;
		try {
			const resp = gpuState === 'on'
				? await api.pauseGpu()
				: await api.resumeGpu();
			if ((resp as Record<string, unknown>).gpu_state) {
				gpuState = (resp as Record<string, unknown>).gpu_state as GpuStateValue;
			}
		} catch {
			/* silent */
		} finally {
			gpuToggling = false;
		}
	}

	function openSearch() {
		uiStore.openPanel('search');
	}

	$effect(() => {
		pollBudget();
		pollGpuStatus();
		// Poll faster during transitions (3s), slower when stable (30s)
		const pollMs = gpuTransitioning ? 3_000 : 30_000;
		const interval = setInterval(() => { pollBudget(); pollGpuStatus(); }, pollMs);
		return () => clearInterval(interval);
	});

	const gpuButtonClass = $derived.by(() => {
		switch (gpuState) {
			case 'on':
				return 'bg-success/20 text-success hover:bg-success/30';
			case 'off':
				return 'bg-warning/20 text-warning hover:bg-warning/30';
			case 'starting':
			case 'stopping':
				return 'bg-warning/20 text-yellow-400 animate-pulse';
		}
	});

	const gpuLabel = $derived.by(() => {
		switch (gpuState) {
			case 'on':
				return 'GPU';
			case 'off':
				return 'GPU OFF';
			case 'starting':
			case 'stopping':
				return 'GPU...';
		}
	});

	const gpuTitle = $derived.by(() => {
		switch (gpuState) {
			case 'on':
				return 'GPU active — click to stop llama container and free VRAM.';
			case 'off':
				return 'GPU off — llama container stopped. Click to start.';
			case 'starting':
				return 'Starting llama container — waiting for model to load...';
			case 'stopping':
				return 'Stopping llama container...';
		}
	});
</script>

<div
	class="flex h-7 shrink-0 items-center justify-between border-t border-border-default bg-bg-primary px-4 text-[11px]"
>
	<!-- Left: connection status -->
	<div class="flex items-center gap-4">
		<div class="flex items-center gap-1.5" title="WebSocket connection: {statusLabels[eventStore.connectionStatus]}">
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

		<button
			class="flex items-center gap-1 rounded px-1.5 py-0.5 text-text-muted transition-colors hover:bg-bg-surface hover:text-text-primary"
			onclick={openSearch}
			title="Search events (hybrid lexical + semantic)"
		>
			<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
				<path stroke-linecap="round" stroke-linejoin="round" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
			</svg>
			<span class="text-[10px] font-semibold tracking-wider">SEARCH</span>
		</button>

		<button
			class="flex items-center gap-1 rounded px-1.5 py-0.5 transition-colors {gpuButtonClass}"
			onclick={toggleGpu}
			disabled={gpuToggling || gpuTransitioning}
			title={gpuTitle}
		>
			<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24" stroke-width="2">
				{#if gpuState === 'on'}
					<polygon points="5,3 19,12 5,21" />
				{:else if gpuState === 'off'}
					<rect x="6" y="4" width="4" height="16" rx="1" />
					<rect x="14" y="4" width="4" height="16" rx="1" />
				{:else}
					<!-- Transitional: spinning circle -->
					<circle cx="12" cy="12" r="9" stroke-dasharray="14 8" />
				{/if}
			</svg>
			<span class="text-[10px] font-semibold tracking-wider">{gpuLabel}</span>
		</button>

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
