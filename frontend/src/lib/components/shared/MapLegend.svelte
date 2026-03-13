<script lang="ts">
	import { mapStore } from '$lib/stores/map.svelte';
	import { satelliteStore } from '$lib/services/satellites.svelte';

	let expanded = $state(false);

	const EVENT_TYPES = [
		{ type: 'conflict_event', color: '#ef4444', label: 'Conflict' },
		{ type: 'thermal_anomaly', color: '#f97316', label: 'Thermal / FIRMS' },
		{ type: 'seismic_event', color: '#eab308', label: 'Seismic' },
		{ type: 'nuclear_event', color: '#f43f5e', label: 'Nuclear' },
		{ type: 'notam_event', color: '#fb923c', label: 'Airspace / NOTAM' },
		{ type: 'gps_interference', color: '#d946ef', label: 'GPS Interference' },
		{ type: 'internet_outage', color: '#a855f7', label: 'Internet Outage' },
		{ type: 'news_article', color: '#22d3ee', label: 'News / Geo' },
		{ type: 'threat_intel', color: '#f472b6', label: 'Threat Intel' },
		{ type: 'fishing_event', color: '#10b981', label: 'Fishing / Maritime' },
		{ type: 'telegram_message', color: '#38bdf8', label: 'Telegram' },
		{ type: 'bgp_leak', color: '#6366f1', label: 'BGP Leak' },
	] as const;
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="absolute bottom-2 left-2 z-10 select-none rounded-lg border border-border-default bg-bg-primary/90 backdrop-blur-sm"
	style="font-size: 10px;"
>
	{#if expanded}
		<div class="overflow-y-auto px-3 py-2" style="min-width: 180px; max-height: calc(100vh - 120px);">
			<div class="sticky top-0 flex items-center justify-between bg-bg-primary/90 pb-1">
				<span class="text-[10px] font-semibold uppercase tracking-wider text-text-secondary">Legend</span>
				<button
					class="rounded p-0.5 text-text-muted hover:text-text-primary"
					onclick={() => (expanded = false)}
					title="Collapse"
					aria-label="Collapse legend"
				>
					<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
					</svg>
				</button>
			</div>

			<!-- Events (clickable toggles) -->
			<div class="mt-2 space-y-0.5">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Events</span>
				{#each EVENT_TYPES as item}
					{@const hidden = mapStore.hiddenEventTypes.has(item.type)}
					<button
						class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
						style="opacity: {hidden ? 0.3 : 1};"
						onclick={() => mapStore.toggleEventType(item.type)}
						title={hidden ? `Show ${item.label}` : `Hide ${item.label}`}
					>
						<span
							class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full"
							style="background-color: {item.color};"
						></span>
						<span
							class="text-text-secondary"
							style={hidden ? 'text-decoration: line-through;' : ''}
						>{item.label}</span>
					</button>
				{/each}
			</div>

			<!-- Show/Hide all -->
			<div class="mt-1.5 flex gap-2">
				<button
					class="text-[9px] text-text-muted hover:text-text-secondary"
					onclick={() => mapStore.showAllEventTypes()}
				>Show All</button>
				<button
					class="text-[9px] text-text-muted hover:text-text-secondary"
					onclick={() => mapStore.hideAllEventTypes()}
				>Hide All</button>
			</div>

			<!-- Incidents -->
			<div class="mt-2 space-y-1">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Incidents</span>
				<div class="flex items-center gap-1.5">
					<span class="relative inline-flex h-3.5 w-3.5 items-center justify-center">
						<span class="absolute h-3.5 w-3.5 rounded-full bg-[#ef4444]/25"></span>
						<span class="relative h-2 w-2 rounded-full border border-white bg-[#ff0000]"></span>
					</span>
					<span class="text-text-secondary">Correlated incident</span>
				</div>
			</div>

			<!-- Tracking -->
			<div class="mt-2 space-y-1">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Tracking</span>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#3b82f6]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">US military</span>
				</div>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#ef4444]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">Russia</span>
				</div>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#22d3ee]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">Israel</span>
				</div>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#10b981]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">Iran</span>
				</div>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#f472b6]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">Other military</span>
				</div>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#64748b]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">Civilian</span>
				</div>
				<div class="flex items-center gap-1.5">
					<svg class="h-3 w-3 text-[#06b6d4]" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2l-5 14h3l2-4 2 4h3z"/></svg>
					<span class="text-text-secondary">Vessel</span>
				</div>
			</div>

			<!-- Severity -->
			<div class="mt-2 space-y-1">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Size = Severity</span>
				<div class="flex items-center gap-2">
					<span class="inline-block h-1.5 w-1.5 rounded-full bg-text-muted/60" title="Low severity"></span>
					<span class="inline-block h-2 w-2 rounded-full bg-text-muted/60" title="Medium severity"></span>
					<span class="inline-block h-2.5 w-2.5 rounded-full bg-text-muted/60" title="High severity"></span>
					<span class="inline-block h-3 w-3 rounded-full bg-text-muted/60" title="Critical severity"></span>
					<span class="text-text-muted">low → critical</span>
				</div>
			</div>

			<!-- Opacity -->
			<div class="mt-2 space-y-1">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Opacity = Age</span>
				<div class="flex items-center gap-1">
					<span class="inline-block h-2 w-4 rounded bg-accent opacity-80" title="Recent event (< 2 hours old)"></span>
					<span class="inline-block h-2 w-4 rounded bg-accent opacity-50" title="Aging event (2-6 hours old)"></span>
					<span class="inline-block h-2 w-4 rounded bg-accent opacity-25" title="Old event (> 6 hours old)"></span>
					<span class="ml-1 text-text-muted">new → 6h+</span>
				</div>
			</div>

			<!-- Overlays -->
			<div class="mt-2 space-y-0.5">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Overlays</span>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.aisZonesVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleAisZones()}
					title={mapStore.aisZonesVisible ? 'Hide AIS Zones' : 'Show AIS Zones'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #06b6d4; background: {mapStore.aisZonesVisible ? '#06b6d4' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.aisZonesVisible ? '' : 'text-decoration: line-through;'}>AIS Zones</span>
				</button>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.heatmapVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleHeatmap()}
					title={mapStore.heatmapVisible ? 'Hide Heatmap' : 'Show Heatmap'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #ef4444; background: {mapStore.heatmapVisible ? '#ef4444' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.heatmapVisible ? '' : 'text-decoration: line-through;'}>Conflict Heatmap</span>
				</button>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.impactSitesVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleImpactSites()}
					title={mapStore.impactSitesVisible ? 'Hide Impact Sites' : 'Show Impact Sites'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #f97316; background: {mapStore.impactSitesVisible ? '#f97316' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.impactSitesVisible ? '' : 'text-decoration: line-through;'}>Impact Sites</span>
				</button>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.basesVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleBases()}
					title={mapStore.basesVisible ? 'Hide Military Bases' : 'Show Military Bases'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #fbbf24; background: {mapStore.basesVisible ? '#fbbf24' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.basesVisible ? '' : 'text-decoration: line-through;'}>Military Bases</span>
				</button>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {satelliteStore.visible ? 1 : 0.3};"
					onclick={() => satelliteStore.toggle()}
					title={satelliteStore.visible ? 'Hide FIRMS Satellites' : 'Show FIRMS Satellites'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-full border"
						style="border-color: #22d3ee; background: {satelliteStore.visible ? '#ffffff' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={satelliteStore.visible ? '' : 'text-decoration: line-through;'}>FIRMS Satellites</span>
				</button>
			</div>

			<!-- Airspace -->
			<div class="mt-2 space-y-0.5">
				<span class="text-[9px] font-semibold uppercase tracking-wider text-text-muted">Airspace</span>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.firBoundariesVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleFirBoundaries()}
					title={mapStore.firBoundariesVisible ? 'Hide FIR Boundaries' : 'Show FIR Boundaries'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #6b7280; background: {mapStore.firBoundariesVisible ? '#6b7280' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.firBoundariesVisible ? '' : 'text-decoration: line-through;'}>FIR Boundaries</span>
				</button>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.restrictedAirspaceVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleRestrictedAirspace()}
					title={mapStore.restrictedAirspaceVisible ? 'Hide Restricted Airspace' : 'Show Restricted Airspace'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #ef4444; background: {mapStore.restrictedAirspaceVisible ? '#ef4444' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.restrictedAirspaceVisible ? '' : 'text-decoration: line-through;'}>Restricted Airspace</span>
				</button>
				<button
					class="flex w-full items-center gap-1.5 rounded px-0.5 py-0.5 text-left hover:bg-white/5"
					style="opacity: {mapStore.notamAreasVisible ? 1 : 0.3};"
					onclick={() => mapStore.toggleNotamAreas()}
					title={mapStore.notamAreasVisible ? 'Hide NOTAM Areas' : 'Show NOTAM Areas'}
				>
					<span class="inline-block h-2.5 w-2.5 flex-shrink-0 rounded-sm border"
						style="border-color: #fb923c; background: {mapStore.notamAreasVisible ? '#fb923c' : 'transparent'}; opacity: 0.6;"></span>
					<span class="text-text-secondary"
						style={mapStore.notamAreasVisible ? '' : 'text-decoration: line-through;'}>NOTAM Areas</span>
				</button>
			</div>
		</div>
	{:else}
		<button
			class="flex items-center gap-1.5 px-2.5 py-1.5 text-text-muted hover:text-text-secondary"
			onclick={() => (expanded = true)}
			title="Show map legend"
			aria-label="Show map legend"
		>
			<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
				<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 20l-5.447-2.724A1 1 0 013 16.382V5.618a1 1 0 011.447-.894L9 7m0 13l6-3m-6 3V7m6 10l4.553 2.276A1 1 0 0021 18.382V7.618a1 1 0 00-.553-.894L15 4m0 13V4m0 0L9 7" />
			</svg>
			<span class="text-[10px] font-medium">Legend</span>
		</button>
	{/if}
</div>
