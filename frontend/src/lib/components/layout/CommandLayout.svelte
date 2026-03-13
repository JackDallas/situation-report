<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { eventStore } from '$lib/stores/events.svelte';
	import { situationsStore } from '$lib/stores/situations.svelte';
	import { sourceStore } from '$lib/stores/sources.svelte';
	import AlertBanner from '$lib/components/shared/AlertBanner.svelte';
	import StatusBar from '$lib/components/shared/StatusBar.svelte';
	import AlertsPanel from '$lib/components/panels/AlertsPanel.svelte';
	import MapPanel from '$lib/components/panels/MapPanel.svelte';
	import NewsPanel from '$lib/components/panels/NewsPanel.svelte';
	import SituationReportsPanel from '$lib/components/panels/SituationReportsPanel.svelte';
	import EventDetailDrawer from '$lib/components/panels/EventDetailDrawer.svelte';
	import SituationDrawer from '$lib/components/panels/SituationDrawer.svelte';
	import PositionDetailPane from '$lib/components/panels/PositionDetailPane.svelte';
	import MapLegend from '$lib/components/shared/MapLegend.svelte';

	const eventsPerMinute = $derived.by(() => {
		const fiveMinAgo = Date.now() - 300000;
		const recent = eventStore.events.filter((e) => {
			const ts = e.event_time;
			if (!ts) return false;
			try {
				return new Date(ts).getTime() > fiveMinAgo;
			} catch {
				return false;
			}
		}).length;
		return Math.round(recent / 5);
	});

	onMount(() => {
		sourceStore.startPolling();
	});

	onDestroy(() => {
		sourceStore.stopPolling();
	});

	// Sync drawer state: when selectedEvent changes externally (e.g. from map popup), open the right panel
	$effect(() => {
		if (eventStore.selectedEvent) {
			uiStore.openPanel('event-detail');
		}
	});

	$effect(() => {
		if (situationsStore.selectedSituation) {
			uiStore.openPanel('situation-detail');
		}
	});

	$effect(() => {
		if (uiStore.selectedPosition) {
			uiStore.openPanel('position-detail');
		}
	});
</script>

<div class="flex h-full flex-col overflow-hidden">
	<!-- Alert Banner -->
	<AlertBanner />

	<!-- 3-zone layout -->
	<div class="flex min-h-0 flex-1">
		<!-- LEFT SIDEBAR -->
		<div class="flex w-[360px] shrink-0 flex-col border-r border-border-default">
			<AlertsPanel />
		</div>

		<!-- CENTER: Map -->
		<div class="relative min-w-0 flex-1">
			<MapPanel />
			<MapLegend />
		</div>

		<!-- RIGHT SIDEBAR (collapsible) -->
		{#if !uiStore.rightCollapsed}
			<div class="flex w-[400px] shrink-0 flex-col border-l border-border-default">
				{#if uiStore.rightPanel === 'event-detail'}
					<EventDetailDrawer />
				{:else if uiStore.rightPanel === 'situation-detail'}
					<SituationDrawer />
				{:else if uiStore.rightPanel === 'position-detail'}
					<PositionDetailPane />
				{:else if uiStore.rightPanel === 'news'}
					<NewsPanel />
				{:else}
					<SituationReportsPanel />
				{/if}
			</div>
		{/if}

		<!-- Collapse/Expand toggle -->
		<button
			class="flex w-5 shrink-0 items-center justify-center border-l border-border-default bg-bg-primary text-text-muted hover:bg-bg-surface hover:text-text-primary"
			onclick={() => uiStore.toggleRight()}
			title={uiStore.rightCollapsed ? 'Show right panel' : 'Hide right panel'}
			aria-label={uiStore.rightCollapsed ? 'Show right panel' : 'Hide right panel'}
		>
			<svg
				class="h-3.5 w-3.5 transition-transform {uiStore.rightCollapsed ? 'rotate-180' : ''}"
				fill="none"
				stroke="currentColor"
				viewBox="0 0 24 24"
			>
				<path
					stroke-linecap="round"
					stroke-linejoin="round"
					stroke-width="2"
					d="M9 5l7 7-7 7"
				/>
			</svg>
		</button>
	</div>

	<!-- Status Bar -->
	<StatusBar sources={sourceStore.sources} {eventsPerMinute} />
</div>
