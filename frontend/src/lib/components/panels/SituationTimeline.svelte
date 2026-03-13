<script lang="ts">
	import { clockStore } from '$lib/stores/clock.svelte';
	import { getTypeColor, formatAbsoluteTime } from '$lib/services/event-display';
	import type { SituationEvent, EvidenceRef } from '$lib/types/events';

	interface Props {
		events?: SituationEvent[];
		evidence?: EvidenceRef[];
	}

	let { events = [], evidence = [] }: Props = $props();

	// Merge events and evidence into timeline nodes
	interface TimelineNode {
		time: string;
		label: string;
		sourceType: string;
		eventType: string;
		severity?: string;
		role?: string;
	}

	const nodes = $derived.by(() => {
		const result: TimelineNode[] = [];

		for (const ev of evidence) {
			result.push({
				time: ev.event_time,
				label: ev.title ?? `${ev.event_type.replace(/_/g, ' ')}`,
				sourceType: ev.source_type,
				eventType: ev.event_type,
				role: ev.role,
			});
		}

		for (const ev of events) {
			result.push({
				time: ev.event_time,
				label: ev.title ?? `${ev.event_type.replace(/_/g, ' ')}`,
				sourceType: ev.source_type,
				eventType: ev.event_type,
				severity: ev.severity,
			});
		}

		result.sort((a, b) => new Date(a.time).getTime() - new Date(b.time).getTime());
		return result;
	});
</script>

{#if nodes.length > 0}
	<div class="relative ml-3 border-l border-border-default pl-4">
		{#each nodes as node}
			{@const colors = getTypeColor(node.eventType)}
			<div class="relative mb-3 last:mb-0">
				<!-- Dot on the timeline line -->
				<div class="absolute -left-[21px] top-1 h-2.5 w-2.5 rounded-full border-2 border-bg-primary {colors.bg}" title="{colors.label} event from {node.sourceType}"></div>

				<!-- Content -->
				<div class="flex items-start gap-1.5">
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-1">
							<span class="rounded px-1 py-0.5 text-[9px] font-medium {colors.bg} {colors.text}">
								{colors.label}
							</span>
							<span class="text-[9px] text-text-muted">{node.sourceType}</span>
							{#if node.role}
								<span class="rounded px-1 py-0.5 text-[9px] {node.role === 'trigger' ? 'bg-alert/20 text-alert' : 'bg-bg-surface text-text-muted'}" title="Evidence role: {node.role === 'trigger' ? 'Trigger — initial event that triggered correlation' : node.role}">
									{node.role}
								</span>
							{/if}
						</div>
						<p class="mt-0.5 line-clamp-2 text-[11px] leading-relaxed text-text-secondary">
							{node.label}
						</p>
					</div>
					<span class="shrink-0 text-[10px] text-text-muted" title={node.time}>
						{formatAbsoluteTime(node.time, clockStore.now)}
					</span>
				</div>
			</div>
		{/each}
	</div>
{:else}
	<p class="text-xs text-text-muted">No timeline data</p>
{/if}
