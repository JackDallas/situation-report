<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
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
	import { getOutlink, getEventDetails } from '$lib/services/outlinks';

	const event = $derived(eventStore.selectedEvent);
	const typeColor = $derived(event ? getTypeColor(event.event_type) : null);
	const sevColor = $derived(event ? getSeverityColor(event.severity) : null);
	const outlink = $derived(event ? getOutlink(event) : null);
	const details = $derived(event ? getEventDetails(event) : []);
	const summary = $derived(event ? getEventSummary(event) : '');

	let showPayload = $state(false);

	function close() {
		eventStore.selectedEvent = null;
		showPayload = false;
		uiStore.openPanel('sitreps');
	}

	function flyTo() {
		if (event?.latitude != null && event?.longitude != null) {
			mapStore.flyTo(event.longitude, event.latitude);
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape' && event) close();
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if event}
	<div class="flex h-full flex-col">
		<!-- Header -->
		<div class="flex items-center gap-2 border-b border-border-default px-4 py-3">
			{#if typeColor}
				<span class="rounded px-1.5 py-0.5 text-[10px] font-medium {typeColor.bg} {typeColor.text}" title="Event type: {event.event_type}">
					{typeColor.label}
				</span>
			{/if}
			{#if sevColor}
				<span class="rounded px-1.5 py-0.5 text-[10px] font-medium {sevColor.badge}" title="Severity: {event.severity}">
					{event.severity.toUpperCase()}
				</span>
			{/if}
			<button
				onclick={close}
				class="ml-auto rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
				title="Close (Esc)"
				aria-label="Close event detail drawer"
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
			<!-- Summary -->
			<p class="text-sm font-medium leading-relaxed text-text-primary">{summary}</p>

			<!-- Source -->
			<p class="mt-1 text-xs text-text-muted">{event.source_type}</p>

			<!-- Timestamp -->
			<div class="mt-2 text-xs text-text-secondary" title={formatFullTimestamp(event.event_time)}>
				<span class="font-mono">{formatFullTimestamp(event.event_time)}</span>
				<span class="text-text-muted">({formatTimestamp(event.event_time, clockStore.now)})</span>
			</div>

			<!-- Key fields -->
			{#if details.length > 0}
				<div class="mt-4 space-y-1.5">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted"
						>Details</span
					>
					{#each details as field}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">{field.label}</span>
							<span class="text-text-secondary">{field.value}</span>
						</div>
					{/each}
				</div>
			{/if}

			<!-- NOTAM Decoded Info -->
			{#if event.event_type === 'notam_event' && event.payload?.qcode_description}
				{@const p = event.payload}
				{@const isRoutine = p.is_routine === true}
				<div class="mt-4 rounded border border-border-default bg-bg-card p-3">
					<div class="flex items-center gap-2 mb-2">
						<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">NOTAM Decode</span>
						{#if isRoutine}
							<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-emerald-500/15 text-emerald-400" title="Routine NOTAM — standard advisory, no unusual significance">Routine</span>
						{:else}
							<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-amber-500/15 text-amber-400" title="Non-routine NOTAM — may indicate unusual activity or restrictions">Potentially Significant</span>
						{/if}
					</div>
					<div class="space-y-1.5">
						<div class="flex gap-2 text-xs">
							<span class="w-20 flex-shrink-0 text-text-muted">Category</span>
							<span class="font-medium text-text-primary">{p.qcode_category}</span>
						</div>
						<div class="flex gap-2 text-xs">
							<span class="w-20 flex-shrink-0 text-text-muted">Meaning</span>
							<span class="text-text-secondary">{p.qcode_description}</span>
						</div>
						{#if p.decoded_text && p.decoded_text !== p.text}
							<div class="mt-2 pt-2 border-t border-border-default">
								<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Expanded Text</span>
								<p class="mt-1 text-xs leading-relaxed text-text-secondary">{p.decoded_text}</p>
							</div>
						{/if}
						{#if p.significance}
							<div class="mt-2 pt-2 border-t border-border-default">
								<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Significance</span>
								<p class="mt-1 text-xs leading-relaxed {isRoutine ? 'text-text-muted' : 'text-amber-400/80'}">{p.significance}</p>
							</div>
						{/if}
					</div>
				</div>
			{/if}

			<!-- Outlink -->
			{#if outlink}
				<a
					href={outlink.url}
					target="_blank"
					rel="noopener noreferrer"
					class="mt-4 inline-flex items-center gap-1.5 rounded bg-accent/10 px-3 py-1.5 text-xs font-medium text-accent transition-colors hover:bg-accent/20" title="Open original source in new tab"
				>
					{outlink.label}
					<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path
							stroke-linecap="round"
							stroke-linejoin="round"
							stroke-width="2"
							d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"
						/>
					</svg>
				</a>
			{/if}

			<!-- Coordinates + Fly to -->
			{#if event.latitude != null && event.longitude != null}
				<div class="mt-4 flex items-center gap-2">
					<span class="text-xs text-text-muted">
						{event.latitude.toFixed(4)}, {event.longitude.toFixed(4)}
					</span>
					<button
						onclick={flyTo}
						class="rounded bg-bg-surface px-2 py-1 text-[10px] font-medium text-text-secondary transition-colors hover:bg-bg-card-hover hover:text-text-primary"
						title="Center map on this event's location"
					>
						Fly to
					</button>
				</div>
			{/if}

			<!-- Tags -->
			{#if event.tags && event.tags.length > 0}
				<div class="mt-4 flex flex-wrap gap-1">
					{#each event.tags as tag}
						<span class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted"
							>{tag}</span
						>
					{/each}
				</div>
			{/if}

			<!-- Raw payload -->
			<div class="mt-4 border-t border-border-default pt-3">
				<button
					onclick={() => (showPayload = !showPayload)}
					class="flex items-center gap-1 text-[10px] font-medium uppercase tracking-wider text-text-muted hover:text-text-secondary"
				>
					<svg
						class="h-3 w-3 transition-transform {showPayload ? 'rotate-90' : ''}"
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
					Raw Payload
				</button>
				{#if showPayload}
					<pre
						class="mt-2 max-h-64 overflow-auto rounded bg-bg-card p-2 text-[10px] text-text-secondary">{JSON.stringify(event.payload, null, 2)}</pre>
				{/if}
			</div>
		</div>
	</div>
{/if}
