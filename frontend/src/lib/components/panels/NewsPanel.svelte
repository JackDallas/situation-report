<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';

	type FilterTab = 'all' | 'middle_east' | 'ukraine' | 'cyber';

	let activeFilter = $state<FilterTab>('all');
	let autoScroll = $state(true);
	let scrollContainer: HTMLDivElement | undefined = $state();
	let prevCount = $state(0);

	const newsEvents = $derived(eventStore.events.filter((e) => e.event_type === 'news_article'));

	const filteredEvents = $derived.by(() => {
		if (activeFilter === 'all') return newsEvents;

		return newsEvents.filter((e) => {
			const enrichment = e.payload?.enrichment as
				| { translated_title?: string; topics?: string[]; summary?: string }
				| undefined;
			const title = (
				(enrichment?.translated_title as string) ??
				(e.payload?.title as string) ??
				''
			).toLowerCase();
			const url = ((e.payload?.url as string) ?? '').toLowerCase();
			const region = ((e.payload?.region as string) ?? '').toLowerCase();
			const summary = (
				(enrichment?.summary as string) ??
				(e.payload?.summary as string) ??
				''
			).toLowerCase();
			const topics = (enrichment?.topics ?? []).join(' ').toLowerCase();
			const text = `${title} ${url} ${region} ${summary} ${topics}`;

			switch (activeFilter) {
				case 'middle_east':
					return /middle.east|israel|palestine|gaza|iran|iraq|syria|yemen|lebanon|houthi|hezbollah|hamas|saudi|jordan|egypt|suez/i.test(
						text
					);
				case 'ukraine':
					return /ukraine|russia|kyiv|moscow|donbas|crimea|zelensk|putin|kherson|bakhmut|zaporizhzhia|kursk/i.test(
						text
					);
				case 'cyber':
					return /cyber|hack|breach|malware|ransomware|vulnerability|zero.day|exploit|apt|threat|phishing|ddos|botnet|infosec/i.test(
						text
					);
				default:
					return true;
			}
		});
	});

	const articleCount = $derived(filteredEvents.length);

	// Auto-scroll to top when new events arrive
	$effect(() => {
		const currentCount = filteredEvents.length;
		if (autoScroll && scrollContainer && currentCount > prevCount && prevCount > 0) {
			scrollContainer.scrollTo({ top: 0, behavior: 'smooth' });
		}
		prevCount = currentCount;
	});

	function getToneColor(tone: unknown): string {
		const t = Number(tone);
		if (isNaN(t)) return 'bg-text-muted';
		if (t < -5) return 'bg-alert';
		if (t <= 0) return 'bg-warning';
		return 'bg-success';
	}

	function getToneTextColor(tone: unknown): string {
		const t = Number(tone);
		if (isNaN(t)) return 'text-text-muted';
		if (t < -5) return 'text-alert';
		if (t <= 0) return 'text-warning';
		return 'text-success';
	}

	function getToneValue(tone: unknown): string {
		const t = Number(tone);
		if (isNaN(t)) return '';
		return t.toFixed(1);
	}

	function getDomain(url: unknown): string {
		if (typeof url !== 'string') return '';
		try {
			return new URL(url).hostname.replace('www.', '');
		} catch {
			return '';
		}
	}

	function openArticle(url: unknown) {
		if (typeof url === 'string') {
			window.open(url, '_blank', 'noopener,noreferrer');
		}
	}

	function getRelevanceColor(score: number): string {
		if (score >= 0.7) return 'bg-success';
		if (score >= 0.3) return 'bg-warning';
		return 'bg-text-muted/50';
	}

	const ENTITY_TYPE_COLORS: Record<string, string> = {
		person: 'bg-purple-500/20 text-purple-400',
		organization: 'bg-blue-500/20 text-blue-400',
		location: 'bg-emerald-500/20 text-emerald-400',
		weapon_system: 'bg-red-500/20 text-red-400',
		military_unit: 'bg-orange-500/20 text-orange-400'
	};

	const filters: { key: FilterTab; label: string }[] = [
		{ key: 'all', label: 'All' },
		{ key: 'middle_east', label: 'Mid East' },
		{ key: 'ukraine', label: 'Ukraine' },
		{ key: 'cyber', label: 'Cyber' }
	];

	// Count per filter for badges
	const filterCounts = $derived.by(() => {
		const counts: Record<FilterTab, number> = {
			all: newsEvents.length,
			middle_east: 0,
			ukraine: 0,
			cyber: 0
		};
		for (const e of newsEvents) {
			const enrichment = e.payload?.enrichment as
				| { translated_title?: string; topics?: string[] }
				| undefined;
			const text =
				`${enrichment?.translated_title ?? (e.payload?.title as string) ?? ''} ${(e.payload?.region as string) ?? ''} ${(e.payload?.summary as string) ?? ''} ${(enrichment?.topics ?? []).join(' ')}`.toLowerCase();
			if (
				/middle.east|israel|palestine|gaza|iran|iraq|syria|yemen|lebanon|houthi|hezbollah|hamas|saudi/i.test(
					text
				)
			)
				counts.middle_east++;
			if (
				/ukraine|russia|kyiv|moscow|donbas|crimea|zelensk|putin|kherson|bakhmut/i.test(text)
			)
				counts.ukraine++;
			if (
				/cyber|hack|breach|malware|ransomware|vulnerability|zero.day|exploit|apt|threat|phishing|ddos/i.test(
					text
				)
			)
				counts.cyber++;
		}
		return counts;
	});

	function articleTime(event: { event_time: string; payload?: Record<string, unknown> }): string {
		return (event.payload?.published_at as string) ?? (event.payload?.seendate as string) ?? event.event_time;
	}
</script>

<div class="flex h-full flex-col">
	<!-- Header -->
	<div class="border-b border-border-default px-4 py-2">
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<button
					onclick={() => uiStore.openPanel('sitreps')}
					class="rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
					title="Back to Situation Reports"
					aria-label="Back to Situation Reports"
				>
					<svg class="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M15 19l-7-7 7-7"
						/>
					</svg>
				</button>
				<span class="text-xs font-semibold uppercase tracking-wider text-text-secondary"
					>News Feed</span
				>
				<span class="rounded-full bg-accent/20 px-2 py-0.5 text-xs font-medium text-accent" title="Total news articles in current filter">
					{articleCount}
				</span>
			</div>
			<button
				class="rounded px-2 py-0.5 text-[10px] font-medium transition-colors {autoScroll
					? 'bg-success/20 text-success'
					: 'bg-bg-surface text-text-muted hover:text-text-secondary'}"
				onclick={() => (autoScroll = !autoScroll)}
				title={autoScroll ? 'Auto-scroll ON' : 'Auto-scroll OFF'}
			>
				{autoScroll ? 'AUTO' : 'PAUSED'}
			</button>
		</div>
	</div>

	<!-- Filter Tabs -->
	<div class="flex border-b border-border-default">
		{#each filters as filter}
			<button
				class="flex-1 px-2 py-1.5 text-xs font-medium transition-colors {activeFilter ===
				filter.key
					? 'border-b-2 border-accent text-accent'
					: 'text-text-muted hover:text-text-secondary'}"
				onclick={() => (activeFilter = filter.key)}
			>
				{filter.label}
				{#if filterCounts[filter.key] > 0}
					<span class="ml-0.5 text-[10px] opacity-60">({filterCounts[filter.key]})</span>
				{/if}
			</button>
		{/each}
	</div>

	<!-- Article List -->
	<div class="flex-1 overflow-auto" bind:this={scrollContainer}>
		{#if filteredEvents.length === 0}
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
							d="M19 20H5a2 2 0 01-2-2V6a2 2 0 012-2h10a2 2 0 012 2v1m2 13a2 2 0 01-2-2V7m2 13a2 2 0 002-2V9a2 2 0 00-2-2h-2m-4-3H9M7 16h6M7 8h6v4H7V8z"
						/>
					</svg>
					<p class="text-sm">No news articles yet</p>
					<p class="mt-1 text-xs">Articles will appear as sources report in</p>
				</div>
			</div>
		{:else}
			<div class="divide-y divide-border-default">
				{#each filteredEvents.slice(0, 100) as event}
					{@const rawTitle = (event.payload?.title as string) ?? 'Untitled'}
					{@const enrichment = event.payload?.enrichment as
						| {
								translated_title?: string;
								summary?: string;
								entities?: Array<{ name: string; entity_type: string; role?: string }>;
								topics?: string[];
								relevance_score?: number;
								sentiment?: number;
								original_language?: string;
						  }
						| undefined}
					{@const isEnriched = !!enrichment}
					{@const displayTitle = enrichment?.translated_title ?? rawTitle}
					{@const isTranslated =
						isEnriched && enrichment.original_language && enrichment.original_language !== 'en'}
					{@const relevance = enrichment?.relevance_score ?? -1}
					{@const ts = articleTime(event)}
					<button
						class="group flex w-full cursor-pointer gap-2.5 px-4 py-3 text-left transition-colors hover:bg-bg-card-hover"
						onclick={() => openArticle(event.payload?.url)}
					>
						<!-- Left indicator: relevance dot or tone bar -->
						<div class="mt-0.5 flex flex-shrink-0 flex-col items-center gap-1">
							{#if relevance >= 0}
								<div
									class="h-2.5 w-2.5 rounded-full {getRelevanceColor(relevance)}"
									title="Relevance: {(relevance * 100).toFixed(0)}%"
								></div>
							{:else}
								<div
									class="h-12 w-1 rounded-full {getToneColor(event.payload?.tone)}"
									title="Tone: {getToneValue(event.payload?.tone)}"
								></div>
							{/if}
						</div>

						<!-- Content -->
						<div class="min-w-0 flex-1">
							<div class="flex items-start gap-2">
								<h3
									class="line-clamp-2 flex-1 text-[13px] font-medium leading-snug text-text-primary transition-colors group-hover:text-accent"
									title={isTranslated ? rawTitle : undefined}
								>
									{displayTitle}
								</h3>
								<span class="flex-shrink-0 text-[10px] text-text-muted" title={formatFullTimestamp(ts)}>
									{formatAbsoluteTime(ts, clockStore.now)}
								</span>
							</div>
							{#if isTranslated}
								<span
									class="mt-0.5 inline-block rounded bg-blue-500/10 px-1 py-0.5 text-[9px] font-medium text-blue-400"
									 title="Originally written in {enrichment.original_language?.toUpperCase()}, auto-translated by AI">{enrichment.original_language?.toUpperCase()} translated</span
								>
							{/if}

							{#if enrichment?.summary}
								<p class="mt-1 line-clamp-2 text-[11px] leading-relaxed text-text-muted/80">
									{enrichment.summary}
								</p>
							{:else if event.payload?.summary}
								<p class="mt-1 line-clamp-2 text-xs leading-relaxed text-text-muted/70">
									{event.payload.summary}
								</p>
							{/if}

							<!-- Entity chips -->
							{#if enrichment?.entities && enrichment.entities.length > 0}
								<div class="mt-1.5 flex flex-wrap gap-1">
									{#each enrichment.entities.slice(0, 5) as entity}
										<span
											class="rounded px-1 py-0.5 text-[9px] font-medium {ENTITY_TYPE_COLORS[
												entity.entity_type
											] ?? 'bg-bg-surface text-text-muted'}"
											title="{entity.entity_type}: {entity.name}{entity.role
												? ` (${entity.role})`
												: ''}"
										>
											{entity.name}
										</span>
									{/each}
									{#if enrichment.entities.length > 5}
										<span class="text-[9px] text-text-muted"
											>+{enrichment.entities.length - 5}</span
										>
									{/if}
								</div>
							{/if}

							<div class="mt-1.5 flex items-center gap-2 text-xs text-text-muted">
								{#if getDomain(event.payload?.url)}
									<span class="truncate font-medium text-text-secondary"
										>{getDomain(event.payload?.url)}</span
									>
									<span class="text-text-muted/50">|</span>
								{:else if event.source_type}
									<span class="font-medium text-text-secondary">{event.source_type}</span>
									<span class="text-text-muted/50">|</span>
								{/if}

								{#if !isEnriched && event.payload?.tone !== undefined}
									<span
										class="inline-flex items-center gap-1 rounded px-1 py-0.5 text-[10px] font-medium {getToneColor(event.payload.tone)}/20 {getToneTextColor(event.payload.tone)}"
									>
										{getToneValue(event.payload.tone)}
									</span>
								{/if}

								<!-- Topic chips -->
								{#if enrichment?.topics}
									{#each enrichment.topics.slice(0, 2) as topic}
										<span
											class="rounded bg-bg-surface px-1 py-0.5 text-[9px] text-text-muted"
										>
											{topic}
										</span>
									{/each}
								{/if}

								<span class="ml-auto flex-shrink-0 opacity-60"
									>{formatTimestamp(ts, clockStore.now)}</span
								>
							</div>
						</div>

						<!-- External link icon -->
						<div class="mt-1 flex-shrink-0 opacity-0 transition-opacity group-hover:opacity-100" title="Open article in new tab">
							<svg
								class="h-3.5 w-3.5 text-text-muted"
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
						</div>
					</button>
				{/each}
			</div>
		{/if}
	</div>
</div>
