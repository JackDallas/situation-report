<script lang="ts">
	import { api } from '$lib/services/api';
	import type { SearchResult } from '$lib/services/api';
	import { eventStore } from '$lib/stores/events.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		getTypeColor,
		getSeverityColor,
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';

	let query = $state('');
	let results = $state<SearchResult[]>([]);
	let loading = $state(false);
	let searched = $state(false);
	let error = $state<string | null>(null);

	async function doSearch() {
		const q = query.trim();
		if (!q) return;
		loading = true;
		error = null;
		searched = true;
		try {
			results = await api.searchEvents(q, 30);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Search failed';
			results = [];
		} finally {
			loading = false;
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') doSearch();
	}

	function handleResultClick(result: SearchResult) {
		// Build a minimal SituationEvent-compatible object to select in eventStore
		const event = eventStore.events.find(
			(ev) =>
				ev.source_type === result.source_type &&
				ev.source_id === result.source_id &&
				ev.event_time === result.event_time
		);
		if (event) {
			eventStore.selectedEvent = event;
			uiStore.openPanel('event-detail');
		}
	}
</script>

<div class="flex h-full flex-col">
	<!-- Header -->
	<div class="border-b border-border-default px-4 py-2">
		<div class="flex items-center gap-2">
			<button
				onclick={() => uiStore.openDefault()}
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
				>Search</span
			>
			{#if results.length > 0}
				<span
					class="rounded-full bg-accent/20 px-2 py-0.5 text-xs font-medium text-accent"
					title="Number of search results"
				>
					{results.length}
				</span>
			{/if}
		</div>
	</div>

	<!-- Search Input -->
	<div class="border-b border-border-default px-4 py-2">
		<div class="flex gap-2">
			<input
				type="text"
				bind:value={query}
				onkeydown={handleKeydown}
				placeholder="Search events..."
				class="min-w-0 flex-1 rounded border border-border-default bg-bg-surface px-2.5 py-1.5 font-mono text-xs text-text-primary placeholder:text-text-muted/50 focus:border-accent focus:outline-none"
			/>
			<button
				onclick={doSearch}
				disabled={loading || !query.trim()}
				class="rounded bg-accent/20 px-3 py-1.5 text-xs font-semibold text-accent transition-colors hover:bg-accent/30 disabled:opacity-40"
			>
				{#if loading}
					...
				{:else}
					Search
				{/if}
			</button>
		</div>
	</div>

	<!-- Results -->
	<div class="flex-1 overflow-auto">
		{#if !searched}
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
							d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"
						/>
					</svg>
					<p class="text-xs">Hybrid search</p>
					<p class="mt-1 text-[10px]">Search across all events by text</p>
				</div>
			</div>
		{:else if loading}
			<div class="flex h-32 items-center justify-center">
				<span class="text-xs text-text-muted">Searching...</span>
			</div>
		{:else if error}
			<div class="p-4 text-xs text-alert">{error}</div>
		{:else if results.length === 0}
			<div class="flex h-32 items-center justify-center text-text-muted">
				<div class="text-center">
					<p class="text-xs">No results</p>
					<p class="mt-1 text-[10px]">Try different keywords</p>
				</div>
			</div>
		{:else}
			<div class="divide-y divide-border-default">
				{#each results as result (result.source_type + ':' + result.source_id + ':' + result.event_time)}
					{@const colors = getTypeColor(result.event_type ?? '')}
					{@const severity = result.severity
						? getSeverityColor(result.severity)
						: null}
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<div
						class="cursor-pointer border-l-[3px] px-3 py-2 transition-colors hover:bg-bg-card-hover {colors.border}"
						onclick={() => handleResultClick(result)}
					>
						<div class="flex items-center gap-1.5">
							<span
								class="rounded px-1.5 py-0.5 text-[10px] font-medium {colors.bg} {colors.text}"
							>
								{colors.label}
							</span>
							<span
								class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted"
							>
								{result.source_type}
							</span>
							{#if severity}
								<span
									class="rounded px-1 py-0.5 text-[9px] font-bold {severity.badge}"
								>
									{(result.severity ?? '').toUpperCase()}
								</span>
							{/if}
							<span
								class="ml-auto flex-shrink-0 text-[10px] text-text-muted"
								title={formatFullTimestamp(result.event_time)}
							>
								{formatAbsoluteTime(result.event_time, clockStore.now)}
								<span class="opacity-60"
									>{formatTimestamp(result.event_time, clockStore.now)}</span
								>
							</span>
						</div>
						<p
							class="mt-1 line-clamp-2 text-[11px] leading-relaxed text-text-secondary"
						>
							{result.title ?? 'Untitled event'}
						</p>
						<div class="mt-0.5 flex items-center gap-2">
							{#if result.region_code}
								<span
									class="rounded bg-bg-surface px-1 py-0.5 text-[9px] text-text-muted"
								>
									{result.region_code}
								</span>
							{/if}
							<span
								class="text-[9px] text-text-muted/60"
								title="Search relevance score"
							>
								{result.match_type}
								{(result.score * 100).toFixed(0)}%
							</span>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>
