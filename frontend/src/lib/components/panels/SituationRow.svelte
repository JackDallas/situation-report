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

	const PHASE_DISPLAY: Record<string, { label: string; class: string }> = {
		active: { label: 'ACTIVE', class: 'bg-red-500/20 text-red-400' },
		developing: { label: 'DEVELOPING', class: 'bg-amber-500/20 text-amber-400' },
		emerging: { label: 'EMERGING', class: 'bg-blue-500/20 text-blue-400' },
		declining: { label: 'DECLINING', class: 'bg-zinc-500/20 text-zinc-400' },
		historical: { label: 'HISTORICAL', class: 'bg-zinc-600/20 text-zinc-500' },
	};

	const phaseInfo = $derived(situation.phase ? PHASE_DISPLAY[situation.phase] : null);

	/** First sentence of narrative as a preview (strips markdown headings) */
	const narrativePreview = $derived.by(() => {
		let text = situation.narrativeText;
		if (!text) return null;
		// Strip markdown headings (# Title, ## Title, etc.)
		text = text.replace(/^#{1,3}\s+[^\n]*\n+/g, '').trim();
		if (!text) return null;
		// Take first sentence or first 120 chars
		const firstSentence = text.match(/^[^.!?]+[.!?]/)?.[0];
		if (firstSentence && firstSentence.length <= 140) return firstSentence;
		return text.slice(0, 120).trim() + '...';
	});
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
		{#if phaseInfo && !isChild}
			<span class="rounded px-1 py-0.5 text-[9px] font-medium {phaseInfo.class}">
				{phaseInfo.label}
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
	{#if narrativePreview}
		<p class="mt-0.5 text-[10px] leading-snug text-text-secondary line-clamp-2">
			{narrativePreview}
		</p>
	{/if}
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
