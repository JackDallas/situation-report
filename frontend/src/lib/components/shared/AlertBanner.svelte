<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { situationsStore } from '$lib/stores/situations.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { getSeverityColor, formatTimestamp } from '$lib/services/event-display';
	import type { Incident } from '$lib/types/events';

	let dismissed = $state<Set<string>>(new Set());
	let timers = $state<Map<string, ReturnType<typeof setTimeout>>>(new Map());

	const visible = $derived.by(() => {
		return eventStore.incidents
			.filter((i) => {
				const sev = i.severity;
				return (sev === 'critical' || sev === 'high') && !dismissed.has(i.id);
			})
			.slice(0, 2);
	});

	function dismiss(id: string) {
		const next = new Set(dismissed);
		next.add(id);
		dismissed = next;
		const timer = timers.get(id);
		if (timer) {
			clearTimeout(timer);
			const nextTimers = new Map(timers);
			nextTimers.delete(id);
			timers = nextTimers;
		}
	}

	function viewIncident(incident: Incident) {
		dismiss(incident.id);
		// Find matching situation and open it
		const situation = situationsStore.situations.find((s) => s.id === `incident:${incident.id}`);
		if (situation) {
			eventStore.selectedEvent = null;
			situationsStore.selectedSituation = situation;
			uiStore.openPanel('situation-detail');
		}
		if (incident.latitude != null && incident.longitude != null) {
			mapStore.flyTo(incident.longitude, incident.latitude, 10);
		}
	}

	// Auto-dismiss after 30s
	$effect(() => {
		for (const incident of visible) {
			if (!timers.has(incident.id)) {
				const timer = setTimeout(() => dismiss(incident.id), 30_000);
				const nextTimers = new Map(timers);
				nextTimers.set(incident.id, timer);
				timers = nextTimers;
			}
		}
	});
</script>

{#if visible.length > 0}
	<div class="flex flex-col gap-1">
		{#each visible as incident (incident.id)}
			{@const sev = getSeverityColor(incident.severity)}
			<div
				class="flex items-center gap-3 border-l-4 px-4 py-2 {incident.severity === 'critical'
					? 'border-l-red-500 bg-red-500/10'
					: 'border-l-orange-500 bg-orange-500/10'}"
			>
				<span
					class="h-2 w-2 shrink-0 animate-pulse rounded-full {incident.severity === 'critical'
						? 'bg-red-500'
						: 'bg-orange-500'}"
					title="{incident.severity === 'critical' ? 'Critical severity incident' : 'High severity incident'}"
				></span>

				<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {sev.badge}" title="{incident.severity === 'critical' ? 'Critical severity — immediate attention required' : 'High severity — prompt attention recommended'}">
					{incident.severity.toUpperCase()}
				</span>

				<span class="min-w-0 flex-1 truncate text-xs font-medium text-text-primary">
					{incident.title}
				</span>

				<span class="shrink-0 text-[10px] text-text-muted">
					{formatTimestamp(incident.first_seen)}
				</span>

				<button
					class="shrink-0 rounded bg-white/10 px-2 py-0.5 text-[10px] font-semibold text-text-primary hover:bg-white/20"
					onclick={() => viewIncident(incident)}
					title="View incident details and fly to location"
				>
					View
				</button>

				<button
					class="shrink-0 text-text-muted hover:text-text-primary"
					onclick={() => dismiss(incident.id)}
					title="Dismiss"
					aria-label="Dismiss alert"
				>
					<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M6 18L18 6M6 6l12 12"
						/>
					</svg>
				</button>
			</div>
		{/each}
	</div>
{/if}
