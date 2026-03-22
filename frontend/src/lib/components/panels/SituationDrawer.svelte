<script lang="ts">
	import { situationsStore } from '$lib/stores/situations.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		getSeverityColor,
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';
	import { getOutlink } from '$lib/services/outlinks';
	import { CATEGORY_COLORS, REGION_LABELS } from '$lib/types/situations';
	import { renderMarkdown, splitMarkdownParagraphs } from '$lib/services/markdown';

	interface NarrativeEntry {
		id: string;
		situation_id: string;
		version: number;
		narrative_text: string;
		model: string;
		tokens_used: number;
		generated_at: string;
	}

	const situation = $derived(situationsStore.selectedSituation);
	const sevColor = $derived(situation ? getSeverityColor(situation.severity) : null);
	const catColor = $derived(situation ? CATEGORY_COLORS[situation.category] : null);

	interface SituationEventRow {
		event_time: string;
		source_type: string;
		source_id: string | null;
		latitude: number | null;
		longitude: number | null;
		entity_id: string | null;
		entity_name: string | null;
		event_type: string | null;
		severity: string | null;
		title: string | null;
		description: string | null;
		payload: Record<string, unknown>;
	}

	let cameras = $state<Record<string, unknown>[]>([]);
	let narratives = $state<NarrativeEntry[]>([]);
	let narrativesExpanded = $state(false);
	let situationEvents = $state<SituationEventRow[]>([]);
	let eventsLoading = $state(false);

	// Sub-situations tree view state
	let subSituationsExpanded = $state(false);
	let expandedTreeGroups = $state<Set<string>>(new Set());
	let showAllSubSituations = $state(false);

	const SUB_SITUATION_CAP = 50;

	interface TreeGroup {
		key: string;
		label: string;
		children: import('$lib/types/situations').Situation[];
	}

	// Auto-collapse when count > 20 (reset when situation changes)
	$effect(() => {
		if (situation) {
			subSituationsExpanded = situation.childIds.length <= 20;
			expandedTreeGroups = new Set();
			showAllSubSituations = false;
		}
	});

	// Group sub-situations into a tree by topic > region > severity
	const subSituationTree = $derived.by((): TreeGroup[] => {
		if (!situation || situation.childIds.length === 0) return [];

		const children: import('$lib/types/situations').Situation[] = [];
		for (const childId of situation.childIds) {
			const child = situationsStore.situationById.get(childId);
			if (child) children.push(child);
		}

		const grouped = new Map<string, import('$lib/types/situations').Situation[]>();

		for (const child of children) {
			// Group key: first topic > region > severity
			let groupKey: string;
			if (child.topics && child.topics.length > 0) {
				groupKey = child.topics[0] ?? 'Unknown';
			} else if (child.region && child.region !== 'global') {
				groupKey = REGION_LABELS[child.region] ?? child.region;
			} else {
				groupKey = child.severity.charAt(0).toUpperCase() + child.severity.slice(1);
			}

			let group = grouped.get(groupKey);
			if (!group) {
				group = [];
				grouped.set(groupKey, group);
			}
			group.push(child);
		}

		// Sort groups by count descending
		const groups: TreeGroup[] = [];
		for (const [key, items] of grouped) {
			groups.push({ key, label: key, children: items });
		}
		groups.sort((a, b) => b.children.length - a.children.length);

		return groups;
	});

	// Total child count for display
	const totalSubSituations = $derived(situation?.childIds.length ?? 0);

	function toggleTreeGroup(key: string) {
		const next = new Set(expandedTreeGroups);
		if (next.has(key)) {
			next.delete(key);
		} else {
			next.add(key);
		}
		expandedTreeGroups = next;
	}

	function phaseColor(phase: string | null | undefined): { badge: string; label: string } {
		switch (phase) {
			case 'emerging':
				return { badge: 'bg-blue-500/20 text-blue-400', label: 'EMERGING' };
			case 'developing':
				return { badge: 'bg-cyan-500/20 text-cyan-400', label: 'DEVELOPING' };
			case 'active':
				return { badge: 'bg-warning/20 text-warning', label: 'ACTIVE' };
			case 'declining':
				return { badge: 'bg-text-muted/20 text-text-muted', label: 'DECLINING' };
			case 'resolved':
				return { badge: 'bg-success/20 text-success', label: 'RESOLVED' };
			case 'historical':
				return { badge: 'bg-text-muted/10 text-text-muted/60', label: 'HISTORICAL' };
			default:
				return { badge: 'bg-bg-surface text-text-muted', label: '' };
		}
	}

	// Fetch cameras when situation changes
	$effect(() => {
		if (situation?.id?.startsWith('cluster:')) {
			const clusterId = situation.id.replace('cluster:', '');
			fetch(`/api/situations/${clusterId}/cameras`)
				.then((r) => (r.ok ? r.json() : []))
				.then((data) => {
					cameras = data;
				})
				.catch(() => {
					cameras = [];
				});
		} else {
			cameras = [];
		}
	});

	// Fetch narratives when situation changes
	$effect(() => {
		if (situation?.id?.startsWith('cluster:')) {
			const clusterId = situation.id.replace('cluster:', '');
			fetch(`/api/situations/${clusterId}/narratives?limit=5`)
				.then((r) => (r.ok ? r.json() : []))
				.then((data) => {
					narratives = Array.isArray(data) ? data : [];
				})
				.catch(() => {
					narratives = [];
				});
		} else {
			narratives = [];
		}
	});

	// Fetch events belonging to this situation and add them to the map
	$effect(() => {
		if (situation?.id?.startsWith('cluster:')) {
			const clusterId = situation.id.replace('cluster:', '');
			eventsLoading = true;
			fetch(`/api/situations/${clusterId}/events?limit=50`)
				.then((r) => (r.ok ? r.json() : []))
				.then((data) => {
					situationEvents = Array.isArray(data) ? data : [];
					eventsLoading = false;
					// Add situation events to map so they're visible when flying to centroid
					for (const evt of situationEvents) {
						if (evt.latitude != null && evt.longitude != null) {
							mapStore.addEventFeature({
								source_type: evt.source_type,
								source_id: evt.source_id,
								event_type: evt.event_type,
								event_time: evt.event_time,
								latitude: evt.latitude,
								longitude: evt.longitude,
								severity: evt.severity,
								title: evt.title,
								entity_name: evt.entity_name,
								payload: evt.payload ?? {},
							} as import('$lib/types/events').SituationEvent);
						}
					}
				})
				.catch(() => {
					situationEvents = [];
					eventsLoading = false;
				});
		} else {
			situationEvents = [];
		}
	});

	function close() {
		situationsStore.selectedSituation = null;
		narrativesExpanded = false;
		subSituationsExpanded = false;
		expandedTreeGroups = new Set();
		showAllSubSituations = false;
		uiStore.openDefault();
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape' && situation) close();
	}

	function navigateToSituation(id: string) {
		const target = situationsStore.situationById.get(id);
		if (target) {
			situationsStore.selectedSituation = target;
			if (target.latitude != null && target.longitude != null) {
				mapStore.flyTo(target.longitude, target.latitude, target.incident ? 10 : 6);
			}
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if situation}
	<div class="flex h-full flex-col">
		<!-- Header -->
		<div class="flex items-center gap-2 border-b border-border-default px-4 py-3">
			<button
				onclick={close}
				class="rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
				title="Back to Situation Reports"
				aria-label="Back to Situation Reports"
			>
				<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M15 19l-7-7 7-7"
					/>
				</svg>
			</button>
			{#if sevColor}
				<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {sevColor.badge}" title="Severity: {situation.severity}">
					{situation.severity.toUpperCase()}
				</span>
			{/if}
			{#if situation.phase}
				{@const phs = phaseColor(situation.phase)}
				<span class="rounded px-1.5 py-0.5 text-[10px] font-medium {phs.badge}" title="Situation lifecycle phase: {situation.phase}">
					{phs.label}
				</span>
			{/if}
			{#if catColor}
				<span class="rounded px-1.5 py-0.5 text-[10px] font-medium {catColor.badge}" title="Category: {situation.category}">
					{situation.category.toUpperCase()}
				</span>
			{/if}
			<button
				onclick={close}
				class="ml-auto rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
				title="Close (Esc)"
				aria-label="Close situation drawer"
			>
				<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path
						stroke-linecap="round"
						stroke-linejoin="round"
						stroke-width="2"
						d="M6 18L18 6M6 6l12 12"
					/>
				</svg>
			</button>
		</div>

		<!-- Body -->
		<div class="flex-1 overflow-auto px-4 py-3">
			<!-- Title -->
			<h3 class="text-sm font-semibold text-text-primary">{situation.title}</h3>

			<!-- Region + time range -->
			<p class="mt-1 text-xs text-text-muted">
				{REGION_LABELS[situation.region] ?? situation.region}
			</p>
			{#if situation.firstSeen}
				<p class="mt-1 text-xs text-text-secondary" title="{formatFullTimestamp(situation.firstSeen)} — {formatFullTimestamp(situation.lastUpdated)}">
					{formatAbsoluteTime(situation.firstSeen, clockStore.now)} <span class="text-text-muted">({formatTimestamp(situation.firstSeen, clockStore.now)})</span>
					&mdash;
					{formatAbsoluteTime(situation.lastUpdated, clockStore.now)} <span class="text-text-muted">({formatTimestamp(situation.lastUpdated, clockStore.now)})</span>
				</p>
			{/if}

			<!-- Parent link (when viewing a child situation) -->
			{#if situation.parentId}
				{@const parent = situationsStore.situationById.get(situation.parentId)}
				{#if parent}
					{@const parentSev = getSeverityColor(parent.severity)}
					<div class="mt-3">
						<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Parent Situation</span>
						<!-- svelte-ignore a11y_click_events_have_key_events -->
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<div
							class="mt-1.5 cursor-pointer rounded border border-border-default bg-bg-card px-3 py-2 transition-colors hover:bg-bg-card-hover"
							onclick={() => navigateToSituation(parent.id)}
						>
							<div class="flex items-center gap-1.5">
								<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {parentSev.badge}">
									{parent.severity.toUpperCase()}
								</span>
								<svg class="h-3 w-3 text-text-muted" fill="none" stroke="currentColor" viewBox="0 0 24 24">
									<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M7 11l5-5m0 0l5 5m-5-5v12" />
								</svg>
								<span class="text-[10px] text-text-muted">Parent</span>
							</div>
							<p class="mt-1 text-[11px] font-medium text-text-primary">{parent.displayTitle ?? parent.title}</p>
						</div>
					</div>
				{/if}
			{/if}

			<!-- Entity chips (backend clusters) -->
			{#if situation.entities?.length}
				<div class="mt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Entities</span>
					<div class="mt-1.5 flex flex-wrap gap-1">
						{#each situation.entities as entity}
							<span class="rounded bg-accent/10 px-1.5 py-0.5 text-[10px] text-accent">{entity}</span>
						{/each}
					</div>
				</div>
			{/if}

			<!-- Topic chips (backend clusters) -->
			{#if situation.topics?.length}
				<div class="mt-2">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Topics</span>
					<div class="mt-1.5 flex flex-wrap gap-1">
						{#each situation.topics as topic}
							<span class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-secondary">{topic}</span>
						{/each}
					</div>
				</div>
			{/if}

			<!-- Overview (backend clusters) -->
			{#if !situation.incident && situation.id.startsWith('cluster:')}
				<div class="mt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Overview</span>
					<div class="mt-1.5 space-y-2">
						<!-- Quick stats row -->
						<div class="flex items-center gap-2">
							<span class="rounded bg-bg-surface px-2 py-1 text-[11px] font-medium text-text-primary">
								{situation.eventCount} events
							</span>
							<span class="rounded bg-bg-surface px-2 py-1 text-[11px] text-text-secondary">
								{situation.sourceCount} source{situation.sourceCount !== 1 ? 's' : ''}
							</span>
							{#if situation.firstSeen}
								<span class="ml-auto text-[10px] text-text-muted" title={formatFullTimestamp(situation.firstSeen)}>
									{formatTimestamp(situation.firstSeen, clockStore.now)}
								</span>
							{/if}
						</div>

						<!-- Entities mentioned -->
						{#if situation.entities?.length}
							<div class="flex flex-wrap gap-1">
								{#each situation.entities.slice(0, 8) as entity}
									<span class="rounded-full border border-border-default bg-bg-card px-2 py-0.5 text-[10px] text-text-secondary">
										{entity}
									</span>
								{/each}
								{#if situation.entities.length > 8}
									<span class="text-[10px] text-text-muted">+{situation.entities.length - 8} more</span>
								{/if}
							</div>
						{/if}

						<!-- Topics -->
						{#if situation.topics?.length}
							<div class="flex flex-wrap gap-1">
								{#each situation.topics.slice(0, 6) as topic}
									<span class="rounded bg-accent/10 px-1.5 py-0.5 text-[9px] text-accent">
										{topic}
									</span>
								{/each}
							</div>
						{/if}
					</div>
				</div>
			{/if}

			<!-- Recent Activity (event titles from the situation) -->
			{#if situation.eventTitles?.length}
				<div class="mt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Recent Activity
					</span>
					<div class="mt-1.5 space-y-1">
						{#each situation.eventTitles.slice(0, 8) as title}
							<p class="flex items-start gap-1.5 text-[10px] leading-snug text-text-secondary">
								<span class="mt-0.5 inline-block h-1.5 w-1.5 flex-shrink-0 rounded-full bg-accent/40"></span>
								{title}
							</p>
						{/each}
						{#if situation.eventTitles.length > 8}
							<p class="text-[9px] text-text-muted">+{situation.eventTitles.length - 8} more</p>
						{/if}
					</div>
				</div>
			{/if}

			<!-- Impact Sites (backend clusters with geo_event type events and centroid) -->
			{#if !situation.incident && situation.id.startsWith('cluster:') && situation.latitude != null && situation.longitude != null}
				{@const geoEvents = situation.events.filter(e => e.event_type === 'geo_event')}
				{#if geoEvents.length > 0}
					<div class="mt-3">
						<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
							Impact Sites ({geoEvents.length})
						</span>
						<div class="mt-1.5">
							<button
								onclick={() => {
									if (situation?.latitude != null && situation?.longitude != null) {
										mapStore.flyTo(situation.longitude, situation.latitude, 8);
									}
								}}
								class="flex items-center gap-1.5 rounded bg-bg-surface px-2.5 py-1.5 text-[10px] font-medium text-text-secondary transition-colors hover:bg-bg-card-hover hover:text-text-primary"
							>
								<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
									<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M17.657 16.657L13.414 20.9a1.998 1.998 0 01-2.827 0l-4.244-4.243a8 8 0 1111.314 0z" />
									<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M15 11a3 3 0 11-6 0 3 3 0 016 0z" />
								</svg>
								Fly to impacts
							</button>
						</div>
					</div>
				{/if}
			{/if}

			<!-- Supplementary Context (backend clusters with Exa web search data) -->
			{#if situation.supplementary?.articles?.length}
				<div class="mt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Supplementary Context
					</span>
					<div class="mt-1.5 space-y-2">
						{#each situation.supplementary.articles as article}
							<div class="border-l-2 border-blue-500/30 py-2 pl-3">
								<a
									href={article.url}
									target="_blank"
									rel="noopener noreferrer"
									class="text-[11px] font-medium text-blue-400 hover:text-blue-300 hover:underline"
								>
									{article.title}
								</a>
								{#if article.snippet}
									<p class="mt-0.5 text-[10px] leading-relaxed text-text-muted">{article.snippet}</p>
								{/if}
								{#if article.published_date}
									<p class="mt-0.5 text-[9px] text-text-muted/60">
										{formatAbsoluteTime(article.published_date, clockStore.now)}
									</p>
								{/if}
							</div>
						{/each}
					</div>
				</div>
			{/if}

			<!-- Narratives (intelligence analysis for this situation) -->
			{#if narratives.length > 0 && narratives[0]}
				{@const latest = narratives[0]}
				<div class="mt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Analysis
					</span>
					<!-- Latest narrative -->
					<div class="mt-1.5 rounded border border-border-default bg-bg-card p-3">
						<div class="narrative-content space-y-2 text-[11px] leading-relaxed text-text-secondary">
							{@html renderMarkdown(latest.narrative_text)}
						</div>
						<div class="mt-2 flex items-center gap-2 text-[9px] text-text-muted/60">
							<span>{latest.model}</span>
							<span>{latest.tokens_used.toLocaleString()} tokens</span>
							<span class="ml-auto" title={formatFullTimestamp(latest.generated_at)}>
								{formatAbsoluteTime(latest.generated_at, clockStore.now)}
							</span>
						</div>
					</div>

					<!-- Older narratives (expandable) -->
					{#if narratives.length > 1}
						<button
							class="mt-2 flex items-center gap-1 text-[10px] text-accent hover:text-accent/80"
							onclick={() => (narrativesExpanded = !narrativesExpanded)}
						>
							<svg
								class="h-3 w-3 transition-transform {narrativesExpanded ? 'rotate-90' : ''}"
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
							{narratives.length - 1} previous version{narratives.length - 1 !== 1 ? 's' : ''}
						</button>
						{#if narrativesExpanded}
							<div class="mt-1.5 space-y-2">
								{#each narratives.slice(1) as entry}
									<div
										class="rounded border border-border-default/50 bg-bg-card/50 p-2.5"
									>
										<div class="flex items-center gap-2">
											<span class="text-[9px] font-medium text-text-muted"
												>v{entry.version}</span
											>
											<span
												class="text-[9px] text-text-muted/60"
												title={formatFullTimestamp(entry.generated_at)}
											>
												{formatAbsoluteTime(entry.generated_at, clockStore.now)}
												<span class="opacity-60"
													>{formatTimestamp(entry.generated_at, clockStore.now)}</span
												>
											</span>
										</div>
										<div
											class="narrative-content mt-1 space-y-1.5 text-[10px] leading-relaxed text-text-muted"
										>
											{@html renderMarkdown(splitMarkdownParagraphs(entry.narrative_text).slice(0, 2).join('\n\n'))}
											{#if splitMarkdownParagraphs(entry.narrative_text).length > 2}
												<p class="text-[9px] text-text-muted/50">...</p>
											{/if}
										</div>
									</div>
								{/each}
							</div>
						{/if}
					{/if}
				</div>
			{/if}

			<!-- Nearby cameras (backend cluster situations) -->
			{#if cameras.length > 0}
				<div class="mt-4 border-t border-border-default pt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Nearby Cameras ({cameras.length})
					</span>
					<div class="mt-2 grid grid-cols-2 gap-2">
						{#each cameras as cam}
							<a
								href={cam.shodan_url as string | undefined}
								target="_blank"
								rel="noopener noreferrer"
								class="group rounded border border-border-default bg-bg-card p-2 transition-colors hover:bg-bg-card-hover"
							>
								<img
									src={cam.screenshot_url as string | undefined}
									alt="Camera at {cam.ip}"
									class="mb-1.5 h-20 w-full rounded object-cover"
									loading="lazy"
								/>
								<p class="text-[10px] font-medium text-text-secondary group-hover:text-text-primary">
									{cam.city ?? cam.country ?? cam.ip}
								</p>
								<p class="text-[9px] text-text-muted">{cam.org ?? ''}</p>
							</a>
						{/each}
					</div>
				</div>
			{/if}

			<!-- Backend cluster: events from API -->
			{#if !situation.incident && situation.id.startsWith('cluster:')}
				<div class="mt-4 border-t border-border-default pt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Events ({situation.eventCount})
					</span>
					{#if eventsLoading}
						<div class="mt-2 flex items-center gap-2 text-[10px] text-text-muted">
							<span class="h-3 w-3 animate-spin rounded-full border border-text-muted border-t-transparent"></span>
							Loading events...
						</div>
					{:else if situationEvents.length > 0}
						<div class="mt-2 space-y-1">
							{#each situationEvents as evt}
								{@const outlink = getOutlink(evt)}
								<div class="rounded px-2 py-1.5 text-[11px] text-text-secondary hover:bg-bg-card-hover">
									<div class="flex items-start gap-2">
										<span class="mt-0.5 shrink-0 rounded bg-bg-surface px-1 py-0.5 text-[9px] font-medium text-text-muted">
											{evt.source_type}
										</span>
									<div class="min-w-0 flex-1">
											{#if outlink}
												<a href={outlink.url} target="_blank" rel="noopener noreferrer" class="line-clamp-2 leading-relaxed text-blue-400 hover:text-blue-300 hover:underline">{evt.title ?? evt.description ?? '(no title)'}</a>
											{:else}
												<p class="line-clamp-2 leading-relaxed">{evt.title ?? evt.description ?? '(no title)'}</p>
											{/if}
											{#if evt.entity_name}
												<span class="text-[9px] text-accent">{evt.entity_name}</span>
											{/if}
										</div>
										<span class="shrink-0 text-[10px] text-text-muted">
											{new Date(evt.event_time).toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit' })}
										</span>
									</div>
								</div>
							{/each}
						</div>
					{:else}
						<p class="mt-2 text-[10px] text-text-muted">No events found</p>
					{/if}
				</div>
			{/if}

			<!-- Sub-situations tree (at bottom, collapsible, grouped) -->
			{#if totalSubSituations > 0}
				<div class="mt-4 border-t border-border-default pt-3">
					<button
						class="flex w-full items-center gap-1.5 text-left"
						onclick={() => (subSituationsExpanded = !subSituationsExpanded)}
					>
						<svg
							class="h-3 w-3 shrink-0 text-text-muted transition-transform {subSituationsExpanded ? 'rotate-90' : ''}"
							fill="none"
							stroke="currentColor"
							viewBox="0 0 24 24"
						>
							<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
						</svg>
						<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
							Sub-situations ({totalSubSituations})
						</span>
					</button>

					{#if subSituationsExpanded}
						<div class="mt-2 space-y-1">
							{#each subSituationTree as group, groupIdx}
								{@const visibleChildren = (() => {
									const cap = showAllSubSituations ? Infinity : SUB_SITUATION_CAP;
									// Count how many items we've already used from earlier groups
									let usedBefore = 0;
									for (let i = 0; i < groupIdx; i++) {
										const g = subSituationTree[i];
										if (g !== undefined) usedBefore += g.children.length;
									}
									const remaining = Math.max(0, cap - usedBefore);
									if (remaining <= 0) return [];
									return group.children.slice(0, remaining);
								})()}
								{#if visibleChildren.length > 0}
									<div>
										<!-- Group header -->
										<button
											class="flex w-full items-center gap-1.5 rounded px-2 py-1.5 text-left hover:bg-bg-surface"
											onclick={() => toggleTreeGroup(group.key)}
										>
											<svg
												class="h-2.5 w-2.5 shrink-0 text-text-muted transition-transform {expandedTreeGroups.has(group.key) ? 'rotate-90' : ''}"
												fill="none"
												stroke="currentColor"
												viewBox="0 0 24 24"
											>
												<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
											</svg>
											<span class="text-[11px] font-medium text-text-secondary">
												{group.label}
											</span>
											<span class="text-[10px] text-text-muted">
												({group.children.length})
											</span>
										</button>

										<!-- Group children -->
										{#if expandedTreeGroups.has(group.key)}
											<div class="ml-4 mt-0.5 space-y-1 border-l border-border-default pl-2">
												{#each visibleChildren as child}
													{@const childSev = getSeverityColor(child.severity)}
													{@const childCat = CATEGORY_COLORS[child.category]}
													<!-- svelte-ignore a11y_click_events_have_key_events -->
													<!-- svelte-ignore a11y_no_static_element_interactions -->
													<div
														class="cursor-pointer rounded border border-border-default bg-bg-card px-3 py-2 transition-colors hover:bg-bg-card-hover"
														onclick={() => navigateToSituation(child.id)}
													>
														<div class="flex items-center gap-1.5">
															<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {childSev.badge}">
																{child.severity.toUpperCase()}
															</span>
															<span class="rounded px-1.5 py-0.5 text-[10px] font-medium {childCat.badge}">
																{child.category.toUpperCase()}
															</span>
															{#if child.sources.length > 1}
																<span class="rounded bg-blue-500/10 px-1 py-0.5 text-[9px] text-blue-400">
																	{child.sources.length} sources
																</span>
															{/if}
															<span class="ml-auto text-[10px] text-text-muted">
																{child.eventCount} events
															</span>
														</div>
														<p class="mt-1 text-[11px] font-medium text-text-primary">{child.displayTitle ?? child.title}</p>
														{#if child.entities?.length}
															<div class="mt-1 flex flex-wrap gap-1">
																{#each child.entities.slice(0, 3) as entity}
																	<span class="rounded bg-accent/10 px-1.5 py-0.5 text-[9px] text-accent">{entity}</span>
																{/each}
																{#if child.entities.length > 3}
																	<span class="text-[9px] text-text-muted">+{child.entities.length - 3}</span>
																{/if}
															</div>
														{/if}
														{#if child.lastUpdated}
															{@const childLastMs = new Date(child.lastUpdated).getTime()}
															{@const childFiveMinAgo = clockStore.now - 5 * 60 * 1000}
															{#if childLastMs > childFiveMinAgo}
																<div class="mt-1">
																	<span class="inline-flex items-center gap-0.5 text-[9px] text-success">
																		<span class="h-1.5 w-1.5 animate-pulse rounded-full bg-success"></span>
																		Growing
																	</span>
																</div>
															{/if}
														{/if}
													</div>
												{/each}
											</div>
										{/if}
									</div>
								{/if}
							{/each}

							<!-- Show all button when capped -->
							{#if !showAllSubSituations && totalSubSituations > SUB_SITUATION_CAP}
								<button
									class="mt-2 w-full rounded bg-bg-surface px-3 py-2 text-center text-[10px] font-medium text-accent hover:bg-bg-card-hover"
									onclick={() => (showAllSubSituations = true)}
								>
									Show all {totalSubSituations} sub-situations
								</button>
							{/if}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	</div>
{/if}
