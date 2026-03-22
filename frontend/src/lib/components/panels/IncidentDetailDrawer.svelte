<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import {
		getTypeColor,
		getSeverityColor,
		formatTimestamp,
		formatAbsoluteTime,
		formatFullTimestamp
	} from '$lib/services/event-display';

	const incident = $derived(eventStore.selectedIncident);
	const sevColor = $derived(incident ? getSeverityColor(incident.severity) : null);

	// Split evidence by role for organized display
	const triggers = $derived(
		incident?.evidence.filter((e) => e.role === 'trigger') ?? []
	);
	const corroborations = $derived(
		incident?.evidence.filter((e) => e.role === 'corroboration') ?? []
	);
	const contextItems = $derived(
		incident?.evidence.filter((e) => e.role === 'context') ?? []
	);

	function close() {
		eventStore.selectedIncident = null;
		uiStore.openPanel('sitreps');
	}

	function flyTo() {
		if (incident?.latitude != null && incident?.longitude != null) {
			mapStore.flyTo(incident.longitude, incident.latitude, 10);
		}
	}

	function handleEvidenceClick(ev: import('$lib/types/events').EvidenceRef) {
		// Try to find the matching event in the event store and select it
		const match = eventStore.events.find(
			(e) =>
				e.source_type === ev.source_type &&
				e.event_type === ev.event_type &&
				e.event_time === ev.event_time
		);
		if (match) {
			eventStore.selectedIncident = null;
			eventStore.selectedEvent = match;
			uiStore.openPanel('event-detail');
			if (match.latitude != null && match.longitude != null) {
				mapStore.flyTo(match.longitude, match.latitude);
			}
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape' && incident) close();
	}

	function roleLabel(role: string): string {
		switch (role) {
			case 'trigger':
				return 'TRIGGER';
			case 'corroboration':
				return 'CORROBORATION';
			case 'context':
				return 'CONTEXT';
			default:
				return role.toUpperCase();
		}
	}

	function roleBadgeClass(role: string): string {
		switch (role) {
			case 'trigger':
				return 'bg-red-500/20 text-red-400';
			case 'corroboration':
				return 'bg-amber-500/20 text-amber-400';
			case 'context':
				return 'bg-sky-500/20 text-sky-400';
			default:
				return 'bg-text-muted/20 text-text-muted';
		}
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if incident}
	<div class="flex h-full flex-col">
		<!-- Header -->
		<div class="flex items-center gap-2 border-b border-border-default px-4 py-3">
			<span
				class="rounded bg-red-500/10 px-1.5 py-0.5 text-[10px] font-medium text-red-400"
			>
				INCIDENT
			</span>
			{#if sevColor}
				<span
					class="rounded px-1.5 py-0.5 text-[10px] font-medium {sevColor.badge}"
					title="Severity: {incident.severity}"
				>
					{incident.severity.toUpperCase()}
				</span>
			{/if}
			{#if incident.confidence}
				<span
					class="text-[10px] text-text-muted"
					title="Correlation confidence"
				>
					{Math.round(incident.confidence * 100)}%
				</span>
			{/if}
			<button
				onclick={close}
				class="ml-auto rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
				title="Close (Esc)"
				aria-label="Close incident detail drawer"
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
			<p class="text-sm font-medium leading-relaxed text-text-primary">
				{incident.display_title ?? incident.title}
			</p>

			<!-- Rule -->
			<div class="mt-1.5 flex items-center gap-2">
				<span class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted">
					{incident.rule_id.replace(/_/g, ' ')}
				</span>
				{#if incident.region_code}
					<span class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted">
						{incident.region_code}
					</span>
				{/if}
			</div>

			<!-- Description -->
			<p class="mt-3 text-[11px] leading-relaxed text-text-secondary">
				{incident.description}
			</p>

			<!-- Timestamps -->
			<div
				class="mt-3 space-y-1 border-t border-border-default pt-3"
			>
				<div class="flex gap-2 text-xs">
					<span class="w-24 flex-shrink-0 text-text-muted">First seen</span>
					<span class="text-text-secondary" title={formatFullTimestamp(incident.first_seen)}>
						{formatAbsoluteTime(incident.first_seen, clockStore.now)}
						<span class="text-text-muted">({formatTimestamp(incident.first_seen, clockStore.now)})</span>
					</span>
				</div>
				<div class="flex gap-2 text-xs">
					<span class="w-24 flex-shrink-0 text-text-muted">Last updated</span>
					<span class="text-text-secondary" title={formatFullTimestamp(incident.last_updated)}>
						{formatAbsoluteTime(incident.last_updated, clockStore.now)}
						<span class="text-text-muted">({formatTimestamp(incident.last_updated, clockStore.now)})</span>
					</span>
				</div>
			</div>

			<!-- Coordinates + Fly to -->
			{#if incident.latitude != null && incident.longitude != null}
				<div class="mt-3 flex items-center gap-2">
					<span class="text-xs text-text-muted">
						{incident.latitude.toFixed(4)}, {incident.longitude.toFixed(4)}
					</span>
					<button
						onclick={flyTo}
						class="rounded bg-bg-surface px-2 py-1 text-[10px] font-medium text-text-secondary transition-colors hover:bg-bg-card-hover hover:text-text-primary"
						title="Center map on this incident's location"
					>
						Fly to
					</button>
				</div>
			{/if}

			<!-- Tags -->
			{#if incident.tags && incident.tags.length > 0}
				<div class="mt-3 flex flex-wrap gap-1">
					{#each incident.tags as tag}
						<span class="rounded bg-bg-surface px-1.5 py-0.5 text-[10px] text-text-muted"
							>{tag}</span
						>
					{/each}
				</div>
			{/if}

			<!-- Evidence Section -->
			{#if incident.evidence.length > 0}
				<div class="mt-4 border-t border-border-default pt-3">
					<div class="flex items-center gap-2">
						<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
							Evidence
						</span>
						<span class="rounded-full bg-bg-surface px-1.5 py-0.5 text-[9px] text-text-muted">
							{incident.evidence.length} source{incident.evidence.length === 1 ? '' : 's'}
						</span>
					</div>

					<!-- Triggers -->
					{#if triggers.length > 0}
						<div class="mt-3">
							<span class="text-[9px] font-semibold uppercase tracking-wider text-red-400/70">
								Triggers
							</span>
							<div class="mt-1 space-y-1">
								{#each triggers as ev}
									{@const evColors = getTypeColor(ev.event_type)}
									<!-- svelte-ignore a11y_click_events_have_key_events -->
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<div
										class="flex cursor-pointer items-start gap-2 rounded border border-border-default bg-bg-card px-2.5 py-2 transition-colors hover:bg-bg-card-hover"
										onclick={() => handleEvidenceClick(ev)}
										title="Click to view source event"
									>
										<div class="min-w-0 flex-1">
											<div class="flex items-center gap-1.5">
												<span class="rounded px-1 py-0.5 text-[9px] font-medium {evColors.bg} {evColors.text}">
													{evColors.label}
												</span>
												<span class="rounded px-1 py-0.5 text-[9px] {roleBadgeClass(ev.role)}">
													{roleLabel(ev.role)}
												</span>
												<span class="ml-auto flex-shrink-0 text-[9px] text-text-muted" title={formatFullTimestamp(ev.event_time)}>
													{formatAbsoluteTime(ev.event_time, clockStore.now)}
												</span>
											</div>
											<p class="mt-1 text-[11px] leading-relaxed text-text-secondary">
												{ev.title ?? ev.event_type}
											</p>
											<span class="mt-0.5 text-[9px] text-text-muted">{ev.source_type}</span>
										</div>
									</div>
								{/each}
							</div>
						</div>
					{/if}

					<!-- Corroborations -->
					{#if corroborations.length > 0}
						<div class="mt-3">
							<span class="text-[9px] font-semibold uppercase tracking-wider text-amber-400/70">
								Corroboration
							</span>
							<div class="mt-1 space-y-1">
								{#each corroborations as ev}
									{@const evColors = getTypeColor(ev.event_type)}
									<!-- svelte-ignore a11y_click_events_have_key_events -->
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<div
										class="flex cursor-pointer items-start gap-2 rounded border border-border-default bg-bg-card px-2.5 py-2 transition-colors hover:bg-bg-card-hover"
										onclick={() => handleEvidenceClick(ev)}
										title="Click to view source event"
									>
										<div class="min-w-0 flex-1">
											<div class="flex items-center gap-1.5">
												<span class="rounded px-1 py-0.5 text-[9px] font-medium {evColors.bg} {evColors.text}">
													{evColors.label}
												</span>
												<span class="rounded px-1 py-0.5 text-[9px] {roleBadgeClass(ev.role)}">
													{roleLabel(ev.role)}
												</span>
												<span class="ml-auto flex-shrink-0 text-[9px] text-text-muted" title={formatFullTimestamp(ev.event_time)}>
													{formatAbsoluteTime(ev.event_time, clockStore.now)}
												</span>
											</div>
											<p class="mt-1 text-[11px] leading-relaxed text-text-secondary">
												{ev.title ?? ev.event_type}
											</p>
											<span class="mt-0.5 text-[9px] text-text-muted">{ev.source_type}</span>
										</div>
									</div>
								{/each}
							</div>
						</div>
					{/if}

					<!-- Context -->
					{#if contextItems.length > 0}
						<div class="mt-3">
							<span class="text-[9px] font-semibold uppercase tracking-wider text-sky-400/70">
								Context
							</span>
							<div class="mt-1 space-y-1">
								{#each contextItems as ev}
									{@const evColors = getTypeColor(ev.event_type)}
									<!-- svelte-ignore a11y_click_events_have_key_events -->
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<div
										class="flex cursor-pointer items-start gap-2 rounded border border-border-default bg-bg-card px-2.5 py-2 transition-colors hover:bg-bg-card-hover"
										onclick={() => handleEvidenceClick(ev)}
										title="Click to view source event"
									>
										<div class="min-w-0 flex-1">
											<div class="flex items-center gap-1.5">
												<span class="rounded px-1 py-0.5 text-[9px] font-medium {evColors.bg} {evColors.text}">
													{evColors.label}
												</span>
												<span class="rounded px-1 py-0.5 text-[9px] {roleBadgeClass(ev.role)}">
													{roleLabel(ev.role)}
												</span>
												<span class="ml-auto flex-shrink-0 text-[9px] text-text-muted" title={formatFullTimestamp(ev.event_time)}>
													{formatAbsoluteTime(ev.event_time, clockStore.now)}
												</span>
											</div>
											<p class="mt-1 text-[11px] leading-relaxed text-text-secondary">
												{ev.title ?? ev.event_type}
											</p>
											<span class="mt-0.5 text-[9px] text-text-muted">{ev.source_type}</span>
										</div>
									</div>
								{/each}
							</div>
						</div>
					{/if}
				</div>
			{/if}

			<!-- Related / Merged info -->
			{#if incident.related_ids.length > 0 || incident.merged_from.length > 0}
				<div class="mt-4 border-t border-border-default pt-3">
					<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Links
					</span>
					{#if incident.related_ids.length > 0}
						<p class="mt-1 text-[10px] text-text-muted">
							{incident.related_ids.length} related incident{incident.related_ids.length === 1 ? '' : 's'}
						</p>
					{/if}
					{#if incident.merged_from.length > 0}
						<p class="mt-1 text-[10px] text-text-muted">
							Merged from {incident.merged_from.length} incident{incident.merged_from.length === 1 ? '' : 's'}
						</p>
					{/if}
				</div>
			{/if}
		</div>
	</div>
{/if}
