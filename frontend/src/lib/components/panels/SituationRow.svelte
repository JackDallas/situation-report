<script lang="ts">
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		getSeverityColor,
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';
	import { CATEGORY_COLORS, REGION_LABELS } from '$lib/types/situations';
	import type { Situation } from '$lib/types/situations';

	interface Props {
		situation: Situation;
		isChild?: boolean;
		recentlyUpdated?: boolean;
		hasChildren?: boolean;
		isExpanded?: boolean;
		onToggleExpand?: () => void;
		onclick: (situation: Situation) => void;
	}

	let {
		situation,
		isChild = false,
		recentlyUpdated = false,
		hasChildren = false,
		isExpanded = false,
		onToggleExpand,
		onclick
	}: Props = $props();

	const sev = $derived(getSeverityColor(situation.severity));
	const catColor = $derived(CATEGORY_COLORS[situation.category]);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="cursor-pointer border-l-[3px] {isChild ? 'py-2 pl-6 pr-3' : 'px-3 py-2'} transition-colors hover:bg-bg-card-hover {sev.border} {situation.incident
		? sev.bg
		: ''} {recentlyUpdated ? 'recently-updated' : ''}"
	onclick={() => onclick(situation)}
>
	<div class="flex items-center gap-1.5">
		{#if hasChildren && !isChild}
			<button
				class="flex h-4 w-4 items-center justify-center rounded text-text-muted hover:text-text-primary"
				aria-label={isExpanded ? 'Collapse child situations' : 'Expand child situations'}
				title={isExpanded ? 'Collapse child situations' : 'Expand child situations'}
				onclick={(e) => {
					e.stopPropagation();
					onToggleExpand?.();
				}}
			>
				<svg
					class="h-3 w-3 transition-transform {isExpanded ? 'rotate-90' : ''}"
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
		{/if}
		<span class="rounded px-1.5 py-0.5 text-[10px] font-bold {sev.badge}">
			{situation.severity.toUpperCase()}
		</span>
		{#if situation.certainty != null}
			<span
				class="text-[9px] {situation.certainty >= 0.7
					? 'font-semibold text-text-primary'
					: situation.certainty >= 0.4
						? 'text-text-secondary'
						: 'text-text-muted'}"
				title="Clustering confidence: {Math.round(situation.certainty * 100)}%"
			>
				({Math.round(situation.certainty * 100)}%)
			</span>
		{/if}
		<span class="rounded px-1.5 py-0.5 text-[10px] font-medium {catColor.badge}">
			{situation.category.toUpperCase()}
		</span>
		{#if situation.incident}
			<span class="rounded bg-red-500/10 px-1.5 py-0.5 text-[10px] font-medium text-red-400">
				INCIDENT
			</span>
		{/if}
		{#if hasChildren && !isChild}
			<span class="rounded bg-bg-surface px-1 py-0.5 text-[9px] text-text-muted">
				{situation.childIds.length} sub
			</span>
		{/if}
		{#if situation.sources.length > 1}
			<span
				class="rounded bg-blue-500/10 px-1 py-0.5 text-[9px] text-blue-400"
				title="Source diversity: {situation.sources.join(', ')}"
			>
				{situation.sources.length} sources
			</span>
		{/if}
		<span
			class="ml-auto flex-shrink-0 text-[10px] text-text-muted"
			title={formatFullTimestamp(situation.lastUpdated)}
		>
			{formatAbsoluteTime(situation.lastUpdated, clockStore.now)}
			<span class="opacity-60">{formatTimestamp(situation.lastUpdated, clockStore.now)}</span>
		</span>
	</div>
	<p class="mt-1 text-[11px] font-medium leading-relaxed text-text-primary">
		{situation.displayTitle ?? situation.title}
	</p>
	{#if situation.entities?.length}
		<div class="mt-1 flex flex-wrap gap-1">
			{#each situation.entities.slice(0, 3) as entity (entity)}
				<span class="rounded bg-accent/10 px-1.5 py-0.5 text-[9px] text-accent">{entity}</span>
			{/each}
			{#if situation.entities.length > 3}
				<span class="text-[9px] text-text-muted">+{situation.entities.length - 3}</span>
			{/if}
		</div>
	{/if}
	<div class="mt-1.5 flex items-center gap-2">
		<span class="text-[10px] text-text-muted">
			{situation.eventCount} events
		</span>
		{#if !isChild}
			<span class="text-[10px] text-text-muted">
				{situation.sourceCount} source{situation.sourceCount !== 1 ? 's' : ''}
			</span>
		{/if}
		<span class="text-[10px] text-text-muted">
			{REGION_LABELS[situation.region] ?? situation.region}
		</span>
		{#if situation.lastUpdated}
			{@const lastMs = new Date(situation.lastUpdated).getTime()}
			{@const fiveMinAgo = clockStore.now - 5 * 60 * 1000}
			{#if lastMs > fiveMinAgo}
				<span class="ml-1 inline-flex items-center gap-0.5 text-[9px] text-success">
					<span class="h-1.5 w-1.5 animate-pulse rounded-full bg-success"></span>
					Growing
				</span>
			{/if}
		{/if}
	</div>
</div>
