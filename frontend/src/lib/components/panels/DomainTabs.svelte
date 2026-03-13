<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { situationsStore } from '$lib/stores/situations.svelte';
	import { uiStore, type DomainTab } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		getTypeColor,
		getSeverityColor,
		getEventSummary,
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';
	import type { SituationEvent } from '$lib/types/events';
	import DataFlowPanel from './DataFlowPanel.svelte';

	const DOMAIN_TYPES: Record<DomainTab, Set<string>> = {
		kinetic: new Set([
			'conflict_event',
			'thermal_anomaly',
			'seismic_event',
			'nuclear_event',
			'gps_interference'
		]),
		cyber: new Set([
			'internet_outage',
			'censorship_event',
			'bgp_leak',
			'threat_intel',
			'shodan_banner',
			'bgp_anomaly',
			'cert_issued'
		]),
		track: new Set(['flight_position', 'vessel_position', 'fishing_event']),
		intel: new Set([
			'news_article',
			'telegram_message',
			'economic_event',
			'notam_event',
			'geo_news',
			'geo_event'
		]),
		flow: new Set()
	};

	const tabs: { key: DomainTab; label: string; desc: string }[] = [
		{ key: 'kinetic', label: 'KINETIC', desc: 'Conflict, thermal, seismic, nuclear, GPS events' },
		{ key: 'cyber', label: 'CYBER', desc: 'Internet outages, BGP leaks, threat intel, Shodan' },
		{ key: 'track', label: 'TRACK', desc: 'Aircraft and vessel position tracking' },
		{ key: 'intel', label: 'INTEL', desc: 'News articles, Telegram, NOTAM, GeoConfirmed' },
		{ key: 'flow', label: 'FLOW', desc: 'Pipeline metrics and event flow statistics' }
	];

	const filteredEvents = $derived.by(() => {
		if (uiStore.domainTab === 'flow') return [];
		const types = DOMAIN_TYPES[uiStore.domainTab];
		return eventStore.events.filter((e) => types.has(e.event_type)).slice(0, 200);
	});

	const tabCounts = $derived.by(() => {
		const counts: Record<DomainTab, number> = { kinetic: 0, cyber: 0, track: 0, intel: 0, flow: 0 };
		for (const e of eventStore.events) {
			for (const [tab, types] of Object.entries(DOMAIN_TYPES)) {
				if (types.has(e.event_type)) {
					counts[tab as DomainTab]++;
					break;
				}
			}
		}
		return counts;
	});

	function handleClick(event: SituationEvent) {
		situationsStore.selectedSituation = null;
		eventStore.selectedEvent = event;
		uiStore.openPanel('event-detail');
		if (event.latitude != null && event.longitude != null) {
			mapStore.flyTo(event.longitude, event.latitude);
		}
	}
</script>

<div class="flex h-full flex-col">
	<!-- Tab bar -->
	<div class="flex shrink-0 border-b border-border-default">
		{#each tabs as tab}
			<button
				class="flex-1 py-1.5 text-[10px] font-semibold tracking-wider transition-colors {uiStore.domainTab ===
				tab.key
					? 'border-b-2 border-accent text-accent'
					: 'text-text-muted hover:text-text-secondary'}"
				onclick={() => (uiStore.domainTab = tab.key)}
				title={tab.key === 'kinetic' ? 'Conflict, thermal, seismic, nuclear, GPS events' : tab.key === 'cyber' ? 'Internet outages, BGP leaks, threat intel, Shodan' : tab.key === 'track' ? 'Aircraft and vessel position tracking' : tab.key === 'intel' ? 'News articles, Telegram, NOTAM, GeoConfirmed' : 'Pipeline metrics and event flow statistics'}
			>
				{tab.label}
				{#if tab.key !== 'flow' && tabCounts[tab.key] > 0}
					<span class="ml-0.5 text-[9px] opacity-60">({tabCounts[tab.key]})</span>
				{/if}
			</button>
		{/each}
	</div>

	<!-- Content area -->
	<div class="flex-1 overflow-auto">
		{#if uiStore.domainTab === 'flow'}
			<DataFlowPanel />
		{:else if filteredEvents.length === 0}
			<div class="flex h-full items-center justify-center text-text-muted">
				<p class="text-xs">No {uiStore.domainTab} events</p>
			</div>
		{:else}
			<div class="divide-y divide-border-default">
				{#each filteredEvents as event}
					{@const colors = getTypeColor(event.event_type)}
					{@const sevColor = event.severity !== 'low' ? getSeverityColor(event.severity) : null}
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<div
						class="flex cursor-pointer items-start gap-2 px-3 py-2 transition-colors hover:bg-bg-card-hover"
						onclick={() => handleClick(event)}
					>
						<!-- Severity dot -->
						<div class="mt-1.5 shrink-0">
							{#if sevColor}
								<span class="inline-block h-2 w-2 rounded-full {sevColor.bg} ring-1 {sevColor.text}"
									title="Severity: {event.severity}"
								></span>
							{:else}
								<span class="inline-block h-2 w-2 rounded-full bg-text-muted/20"></span>
							{/if}
						</div>

						<!-- Content -->
						<div class="min-w-0 flex-1">
							<div class="flex items-center gap-1.5">
								<span class="rounded px-1 py-0.5 text-[10px] font-medium {colors.bg} {colors.text}">
									{colors.label}
								</span>
								<span class="ml-auto shrink-0 text-[10px] text-text-muted" title={formatFullTimestamp(event.event_time)}>
									{formatAbsoluteTime(event.event_time, clockStore.now)} <span class="opacity-60">{formatTimestamp(event.event_time, clockStore.now)}</span>
								</span>
							</div>
							<p class="mt-0.5 line-clamp-1 text-[11px] leading-relaxed text-text-secondary">
								{getEventSummary(event)}
							</p>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>
