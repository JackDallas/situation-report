<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { situationsStore } from '$lib/stores/situations.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		getTypeColor,
		getSeverityColor,
		getEventSummary,
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';
	import { getOutlink } from '$lib/services/outlinks';
	import type { Situation } from '$lib/types/situations';
	import { getRegionCenter } from '$lib/config/regions';
	import type { SituationEvent, Incident } from '$lib/types/events';
	import DomainTabs from './DomainTabs.svelte';
	import SituationRow from './SituationRow.svelte';

	let activeTab = $state<'situations' | 'incidents' | 'feed' | 'domain'>('situations');

	// Track situation updates for visual indicators
	$effect(() => {
		// Access situations to create dependency
		situationsStore.situations;
		situationsStore.trackUpdates();
	});

	/** Check if a situation was recently updated (within last 30 seconds) */
	function isRecentlyUpdated(situationId: string): boolean {
		const updatedAt = situationsStore.updatedAtMap.get(situationId);
		if (!updatedAt) return false;
		return clockStore.now - updatedAt < 30_000;
	}
	let autoScroll = $state(true);
	let scrollContainer: HTMLDivElement | undefined = $state();
	let prevCount = $state(0);
	let expandedParents = $state<Set<string>>(new Set());

	const recentEvents = $derived(eventStore.events.slice(0, 300));
	const recentIncidents = $derived(eventStore.incidents);

	// Unified feed: incidents first (most important), then events
	type FeedItem = { type: 'incident'; data: Incident } | { type: 'event'; data: SituationEvent };

	function feedItemTime(item: FeedItem): string {
		if (item.type === 'incident') return item.data.first_seen;
		if (item.type === 'event') return item.data.event_time;
		return '';
	}

	const feed = $derived.by(() => {
		const items: FeedItem[] = [];
		for (const i of recentIncidents) {
			items.push({ type: 'incident', data: i });
		}
		for (const e of recentEvents) {
			items.push({ type: 'event', data: e });
		}
		items.sort((a, b) => {
			return new Date(feedItemTime(b)).getTime() - new Date(feedItemTime(a)).getTime();
		});
		return items;
	});

	function handleEventClick(event: SituationEvent) {
		situationsStore.selectedSituation = null;
		eventStore.selectedEvent = event;
		uiStore.openPanel('event-detail');
		if (event.latitude != null && event.longitude != null) {
			mapStore.flyTo(event.longitude, event.latitude);
		}
	}

	function handleIncidentClick(incident: Incident) {
		if (incident.latitude != null && incident.longitude != null) {
			mapStore.flyTo(incident.longitude, incident.latitude, 10);
		}
	}

	function handleSituationClick(situation: Situation) {
		eventStore.selectedEvent = null;
		situationsStore.selectedSituation = situation;
		uiStore.openPanel('situation-detail');
		if (situation.latitude != null && situation.longitude != null) {
			mapStore.flyTo(situation.longitude, situation.latitude, situation.incident ? 10 : 6);
		} else if (situation.region) {
			// Fallback: fly to region center when situation has no explicit coordinates
			const center = getRegionCenter(situation.region);
			if (center) {
				mapStore.flyTo(center[0], center[1], 4);
			}
		}
	}

	function handleOutlinkClick(e: MouseEvent, url: string) {
		e.stopPropagation();
		window.open(url, '_blank', 'noopener,noreferrer');
	}

	function isPriority(event: SituationEvent): boolean {
		const type = event.event_type;
		if (type === 'conflict_event') {
			return Number(event.payload?.fatalities) > 0;
		}
		if (type === 'internet_outage') {
			const sev = ((event.payload?.severity as string) ?? '').toLowerCase();
			return sev === 'critical' || sev === 'high';
		}
		if (type === 'threat_intel') {
			return !!event.payload?.adversary;
		}
		return false;
	}

	// Auto-scroll to top when new events arrive
	$effect(() => {
		const currentCount = feed.length;
		if (autoScroll && scrollContainer && currentCount > prevCount && prevCount > 0) {
			scrollContainer.scrollTo({ top: 0, behavior: 'smooth' });
		}
		prevCount = currentCount;
	});
</script>

<div class="flex h-full flex-col">
	<!-- Header -->
	<div class="border-b border-border-default px-3 py-2">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<span class="text-xs font-semibold uppercase tracking-wider text-text-secondary"
					>Alert Feed</span
				>
				{#if eventStore.incidentCount > 0}
					<span
						class="animate-pulse rounded-full bg-red-500/20 px-2 py-0.5 text-[10px] font-bold text-red-400" title="Active correlated incidents detected by pipeline rules"
					>
						{eventStore.incidentCount} incidents
					</span>
				{/if}
				{#if situationsStore.topLevel.length > 0}
					<span class="rounded-full bg-accent/10 px-2 py-0.5 text-[10px] text-accent" title="Clustered event groups across all sources">
						{situationsStore.topLevel.length} situations
					</span>
				{/if}
			</div>
			<button
				class="flex items-center gap-1 rounded px-2 py-1 text-[10px] font-medium transition-colors {autoScroll
					? 'bg-success/20 text-success'
					: 'bg-bg-surface text-text-muted hover:text-text-secondary'}"
				onclick={() => (autoScroll = !autoScroll)}
				title={autoScroll
					? 'Auto-scroll ON - click to pause'
					: 'Auto-scroll OFF - click to resume'}
			>
				{#if autoScroll}
					<div class="h-1.5 w-1.5 animate-pulse rounded-full bg-success"></div>
					LIVE
				{:else}
					<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M10 9v6m4-6v6m7-3a9 9 0 11-18 0 9 9 0 0118 0z"
						/>
					</svg>
					PAUSED
				{/if}
			</button>
		</div>
	</div>

	<!-- Tab Bar -->
	<div class="flex border-b border-border-default">
		<button
			class="flex-1 py-1.5 text-[10px] font-semibold uppercase tracking-wider transition-colors {activeTab ===
			'situations'
				? 'border-b-2 border-accent text-accent'
				: 'text-text-muted hover:text-text-secondary'}"
			onclick={() => (activeTab = 'situations')}
			title="Clustered event groups — correlated situations and incidents"
		>
			Situations{#if situationsStore.topLevel.length > 0}<span
					class="ml-1 rounded-full bg-accent/20 px-1.5 text-[9px]"
					>{situationsStore.topLevel.length}</span
				>{/if}
		</button>
		<button
			class="flex-1 py-1.5 text-[10px] font-semibold uppercase tracking-wider transition-colors {activeTab ===
			'incidents'
				? 'border-b-2 border-red-400 text-red-400'
				: 'text-text-muted hover:text-text-secondary'}"
			onclick={() => (activeTab = 'incidents')}
			title="Cross-source correlated incidents from pipeline rules"
		>
			Incidents{#if eventStore.incidentCount > 0}<span
					class="ml-1 animate-pulse rounded-full bg-red-500/20 px-1.5 text-[9px] text-red-400"
					>{eventStore.incidentCount}</span
				>{/if}
		</button>
		<button
			class="flex-1 py-1.5 text-[10px] font-semibold uppercase tracking-wider transition-colors {activeTab ===
			'feed'
				? 'border-b-2 border-accent text-accent'
				: 'text-text-muted hover:text-text-secondary'}"
			onclick={() => (activeTab = 'feed')}
			title="Raw chronological event stream from all sources"
		>
			Feed
		</button>
		<button
			class="flex-1 py-1.5 text-[10px] font-semibold uppercase tracking-wider transition-colors {activeTab ===
			'domain'
				? 'border-b-2 border-accent text-accent'
				: 'text-text-muted hover:text-text-secondary'}"
			onclick={() => (activeTab = 'domain')}
			title="Events filtered by domain: Kinetic, Cyber, Tracking, Intel"
		>
			Domain
		</button>
	</div>

	<!-- Content area -->
	<div class="min-h-0 flex-1 {activeTab === 'domain' ? 'flex flex-col' : 'overflow-auto'}" bind:this={scrollContainer}>
		{#if activeTab === 'situations'}
			<!-- Situations Tab (tree: parents with expandable children) -->
			{#if situationsStore.topLevel.length === 0}
				<div class="flex h-full items-center justify-center text-text-muted">
					<div class="text-center">
						<svg
							class="mx-auto mb-2 h-8 w-8 text-text-muted/50"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="1.5"
								d="M3.055 11H5a2 2 0 012 2v1a2 2 0 002 2 2 2 0 012 2v2.945M8 3.935V5.5A2.5 2.5 0 0010.5 8h.5a2 2 0 012 2 2 2 0 104 0 2 2 0 012-2h1.064M15 20.488V18a2 2 0 012-2h3.064M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
							/>
						</svg>
						<p class="text-xs">No situations</p>
						<p class="mt-1 text-[10px]">Related events will be grouped here</p>
					</div>
				</div>
			{:else}
				<div class="divide-y divide-border-default">
					{#each situationsStore.topLevel as situation (situation.id)}
						{@const hasChildren = situation.childIds.length > 0}
						{@const isExpanded = expandedParents.has(situation.id)}
						<SituationRow
							{situation}
							{hasChildren}
							{isExpanded}
							recentlyUpdated={isRecentlyUpdated(situation.id)}
							onclick={handleSituationClick}
							onToggleExpand={() => {
								const next = new Set(expandedParents);
								if (isExpanded) next.delete(situation.id);
								else next.add(situation.id);
								expandedParents = next;
							}}
						/>
						{#if hasChildren && isExpanded}
							{#each situation.childIds as childId (childId)}
								{@const child = situationsStore.situationById.get(childId)}
								{#if child}
									<SituationRow
										situation={child}
										isChild={true}
										recentlyUpdated={isRecentlyUpdated(child.id)}
										onclick={handleSituationClick}
									/>
								{/if}
							{/each}
						{/if}
					{/each}
				</div>
			{/if}
		{:else if activeTab === 'incidents'}
			<!-- Incidents Tab — cross-source correlated detections -->
			{#if recentIncidents.length === 0}
				<div class="flex h-full items-center justify-center text-text-muted">
					<div class="text-center">
						<svg class="mx-auto mb-2 h-8 w-8 text-text-muted/50" fill="none" stroke="currentColor" viewBox="0 0 24 24">
							<path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M13 10V3L4 14h7v7l9-11h-7z" />
						</svg>
						<p class="text-xs">No correlated incidents</p>
						<p class="mt-1 text-[10px]">Cross-source correlations (e.g. military strike + thermal + NOTAM) will appear here</p>
					</div>
				</div>
			{:else}
				<div class="divide-y divide-border-default">
					{#each recentIncidents as incident (incident.id)}
						{@const sev = getSeverityColor(incident.severity)}
						<!-- svelte-ignore a11y_click_events_have_key_events -->
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<div
							class="border-l-[3px] px-3 py-2.5 transition-colors hover:bg-bg-card-hover {sev.border} {sev.bg} {incident.latitude != null ? 'cursor-pointer' : ''}"
							onclick={() => handleIncidentClick(incident)}
						>
							<div class="flex items-center gap-1.5">
								<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {sev.badge}">
									{incident.severity.toUpperCase()}
								</span>
								<span class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted">
									{incident.rule_id.replace(/_/g, ' ')}
								</span>
								{#if incident.confidence}
									<span class="text-[9px] text-text-muted" title="Correlation confidence score">
										{Math.round(incident.confidence * 100)}%
									</span>
								{/if}
								<span class="ml-auto flex-shrink-0 text-[10px] text-text-muted" title={formatFullTimestamp(incident.first_seen)}>
									{formatAbsoluteTime(incident.first_seen, clockStore.now)} <span class="opacity-60">{formatTimestamp(incident.first_seen, clockStore.now)}</span>
								</span>
							</div>
							<p class="mt-1 text-[11px] font-medium leading-relaxed text-text-primary">
								{incident.title}
							</p>
							<p class="mt-0.5 text-[11px] leading-relaxed text-text-secondary">
								{incident.description}
							</p>
							{#if incident.region_code}
								<span class="mt-1 inline-block rounded bg-bg-surface px-1.5 py-0.5 text-[9px] text-text-muted">
									{incident.region_code}
								</span>
							{/if}
							{#if incident.evidence && incident.evidence.length > 0}
								<div class="mt-1.5 flex flex-wrap gap-1">
									{#each incident.evidence as ev}
										{@const evColors = getTypeColor(ev.event_type)}
										<span
											class="rounded px-1 py-0.5 text-[9px] {evColors.bg} {evColors.text}"
											title="{ev.role}: {ev.source_type} — {ev.title || ev.event_type}"
										>
											{evColors.label}
											{#if ev.role === 'trigger'}<span class="font-bold">*</span>{/if}
										</span>
									{/each}
								</div>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		{:else if activeTab === 'feed'}
			<!-- Feed Tab -->
			{#if feed.length === 0}
				<div class="flex h-full items-center justify-center text-text-muted">
					<div class="text-center">
						<svg
							class="mx-auto mb-2 h-8 w-8 text-text-muted/50"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path
								stroke-linecap="round"
								stroke-linejoin="round"
								stroke-width="1.5"
								d="M15 17h5l-1.405-1.405A2.032 2.032 0 0118 14.158V11a6.002 6.002 0 00-4-5.659V5a2 2 0 10-4 0v.341C7.67 6.165 6 8.388 6 11v3.159c0 .538-.214 1.055-.595 1.436L4 17h5m6 0v1a3 3 0 11-6 0v-1m6 0H9"
							/>
						</svg>
						<p class="text-xs">No alerts</p>
						<p class="mt-1 text-[10px]">Events from all sources will appear here</p>
					</div>
				</div>
			{:else}
				<div class="divide-y divide-border-default">
					{#each feed as item}
						{#if item.type === 'incident'}
							{@const incident = item.data}
							{@const sev = getSeverityColor(incident.severity)}
							<!-- svelte-ignore a11y_click_events_have_key_events -->
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<div
								class="border-l-[3px] px-3 py-2 transition-colors hover:bg-bg-card-hover {sev.border} {sev.bg} {incident.latitude != null
									? 'cursor-pointer'
									: ''}"
								onclick={() => handleIncidentClick(incident)}
							>
								<div class="flex items-center gap-1.5">
									<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {sev.badge}">
										{incident.severity.toUpperCase()}
									</span>
									<span
										class="rounded bg-red-500/10 px-1.5 py-0.5 text-[10px] font-medium text-red-400"
									>
										INCIDENT
									</span>
									<span
										class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted"
									>
										{incident.rule_id.replace(/_/g, ' ')}
									</span>
									<span class="ml-auto flex-shrink-0 text-[10px] text-text-muted" title={formatFullTimestamp(incident.first_seen)}>
										{formatAbsoluteTime(incident.first_seen, clockStore.now)} <span class="opacity-60">{formatTimestamp(incident.first_seen, clockStore.now)}</span>
									</span>
								</div>
								<p class="mt-1 text-[11px] font-medium leading-relaxed text-text-primary">
									{incident.title}
								</p>
								<p
									class="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-text-secondary"
								>
									{incident.description}
								</p>
								{#if incident.evidence.length > 0}
									<div class="mt-1.5 flex flex-wrap gap-1">
										{#each incident.evidence.slice(0, 5) as ev}
											{@const evColors = getTypeColor(ev.event_type)}
											<span
												class="rounded px-1 py-0.5 text-[9px] {evColors.bg} {evColors.text}"
												title="{ev.role}: {ev.source_type} {ev.event_type}"
											>
												{evColors.label}
												{#if ev.role === 'trigger'}*{/if}
											</span>
										{/each}
										{#if incident.evidence.length > 5}
											<span class="text-[9px] text-text-muted"
												>+{incident.evidence.length - 5}</span
											>
										{/if}
									</div>
								{/if}
							</div>
						{:else if item.type === 'event'}
							{@const event = item.data}
							{@const colors = getTypeColor(event.event_type)}
							{@const priority = isPriority(event)}
							{@const link = getOutlink(event)}
							<!-- svelte-ignore a11y_click_events_have_key_events -->
							<!-- svelte-ignore a11y_no_static_element_interactions -->
							<div
								class="flex cursor-pointer gap-2 border-l-[3px] px-3 py-2 transition-colors hover:bg-bg-card-hover {colors.border} {priority
									? 'bg-alert/5'
									: ''}"
								onclick={() => handleEventClick(event)}
							>
								<div class="min-w-0 flex-1">
									<div class="flex items-center gap-1.5">
										<span
											class="rounded px-1.5 py-0.5 text-[10px] font-medium {colors.bg} {colors.text}"
										>
											{colors.label}
										</span>
										<span
											class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted"
										>
											{event.source_type}
										</span>
										{#if priority}
											<span
												class="animate-pulse rounded bg-alert/20 px-1 py-0.5 text-[9px] font-bold text-alert" title="High priority event — fatalities, critical outage, or active adversary"
											>
												HIGH
											</span>
										{/if}
										{#if link}
											<!-- svelte-ignore a11y_click_events_have_key_events -->
											<!-- svelte-ignore a11y_no_static_element_interactions -->
											<span
												class="cursor-pointer rounded px-1 py-0.5 text-[9px] text-accent hover:text-accent/80"
												title="Open in {link.label}"
												onclick={(e: MouseEvent) => handleOutlinkClick(e, link.url)}
											>
												<svg
													class="inline h-3 w-3"
													fill="none"
													stroke="currentColor"
													viewBox="0 0 24 24"
												>
													<path
														stroke-linecap="round"
														stroke-linejoin="round"
														stroke-width="2"
														d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
													/>
												</svg>
											</span>
										{/if}
										<span class="ml-auto flex-shrink-0 text-[10px] text-text-muted" title={formatFullTimestamp(event.event_time)}>
											{formatAbsoluteTime(event.event_time, clockStore.now)} <span class="opacity-60">{formatTimestamp(event.event_time, clockStore.now)}</span>
										</span>
									</div>
									<p
										class="mt-1 line-clamp-2 text-[11px] leading-relaxed text-text-secondary"
									>
										{getEventSummary(event)}
									</p>
								</div>
							</div>
						{/if}
					{/each}
				</div>
			{/if}
		{:else if activeTab === 'domain'}
			<!-- Domain sub-tabs (Kinetic/Cyber/Track/Intel/Flow) -->
			<DomainTabs />
		{/if}
	</div>
</div>

<style>
	:global(.recently-updated) {
		animation: situation-glow 3s ease-out forwards;
	}

	@keyframes situation-glow {
		0% {
			background-color: rgba(59, 130, 246, 0.15);
			box-shadow: inset 3px 0 0 0 rgba(59, 130, 246, 0.8);
		}
		100% {
			background-color: transparent;
			box-shadow: none;
		}
	}
</style>
