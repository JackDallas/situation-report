<script lang="ts">
	import { onMount } from 'svelte';
	import { digestStore } from '$lib/stores/digest.svelte';
	import { getSeverityColor, getTypeColor } from '$lib/services/event-display';
	import { CATEGORY_COLORS } from '$lib/types/situations';

	let visible = $state(false);
	let autoDismissTimer: ReturnType<typeof setTimeout> | null = null;

	onMount(async () => {
		await digestStore.load();
		if (digestStore.digest) {
			visible = true;
			autoDismissTimer = setTimeout(() => dismiss(), 15_000);
		}
	});

	function dismiss() {
		visible = false;
		if (autoDismissTimer) {
			clearTimeout(autoDismissTimer);
			autoDismissTimer = null;
		}
		digestStore.dismiss();
	}

	const digest = $derived(digestStore.digest);

	const maxBreakdownCount = $derived.by(() => {
		if (!digest) return 1;
		return Math.max(...digest.breakdown.map((b) => b.count), 1);
	});
</script>

{#if visible && digest}
	<!-- Backdrop -->
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="fixed inset-0 z-[100] flex items-center justify-center bg-black/60" onclick={dismiss}>
		<!-- Modal -->
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div
			class="mx-4 w-full max-w-md rounded-lg border border-border-default bg-bg-primary shadow-2xl"
			onclick={(e) => e.stopPropagation()}
		>
			<!-- Header -->
			<div class="border-b border-border-default px-6 py-4">
				<h2 class="text-lg font-bold text-text-primary">While you were away</h2>
				<p class="mt-1 text-xs text-text-muted">
					Last visit {digest.timeSinceLabel} ago
				</p>
			</div>

			<!-- Stats -->
			<div class="grid grid-cols-3 gap-4 border-b border-border-default px-6 py-4">
				<div class="text-center">
					<div class="text-2xl font-bold text-text-primary">{digest.totalEvents}</div>
					<div class="text-[10px] uppercase tracking-wider text-text-muted">Events</div>
				</div>
				<div class="text-center">
					<div class="text-2xl font-bold text-red-400">{digest.totalIncidentLike}</div>
					<div class="text-[10px] uppercase tracking-wider text-text-muted">High/Critical</div>
				</div>
				<div class="text-center">
					<div class="text-2xl font-bold {getSeverityColor(digest.highestSeverity).text}">{digest.highestSeverity.toUpperCase()}</div>
					<div class="text-[10px] uppercase tracking-wider text-text-muted">Peak Severity</div>
				</div>
			</div>

			<!-- Top Events -->
			{#if digest.topEvents.length > 0}
				<div class="border-b border-border-default px-6 py-3">
					<h3 class="mb-2 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						Top Events
					</h3>
					<div class="space-y-2">
						{#each digest.topEvents as event}
							{@const sevColor = getSeverityColor(event.severity)}
							{@const typeColor = getTypeColor(event.event_type)}
							<div class="flex items-start gap-2">
								<span class="mt-0.5 rounded px-1 py-0.5 text-[9px] font-bold {sevColor.badge}">
									{event.severity.toUpperCase()}
								</span>
								<div class="min-w-0 flex-1">
									<p class="line-clamp-1 text-xs text-text-primary">{event.title}</p>
									<span class="text-[10px] {typeColor.text}">{typeColor.label}</span>
								</div>
							</div>
						{/each}
					</div>
				</div>
			{/if}

			<!-- Category Breakdown -->
			{#if digest.breakdown.length > 0}
				<div class="border-b border-border-default px-6 py-3">
					<h3 class="mb-2 text-[10px] font-semibold uppercase tracking-wider text-text-muted">
						By Category
					</h3>
					<div class="space-y-1.5">
						{#each digest.breakdown as item}
							{@const catColor = CATEGORY_COLORS[item.category]}
							<div class="flex items-center gap-2">
								<span class="w-20 text-xs capitalize {catColor?.text ?? 'text-text-secondary'}">
									{item.category}
								</span>
								<div class="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-bg-surface">
									<div
										class="h-full rounded-full {catColor?.bg ?? 'bg-accent/30'}"
										style="width: {(item.count / maxBreakdownCount) * 100}%; min-width: 4px;"
									></div>
								</div>
								<span class="w-8 text-right text-[10px] text-text-muted">{item.count}</span>
							</div>
						{/each}
					</div>
				</div>
			{/if}

			<!-- Dismiss -->
			<div class="flex justify-end px-6 py-3">
				<button
					class="rounded bg-accent/20 px-4 py-1.5 text-xs font-medium text-accent transition-colors hover:bg-accent/30"
					onclick={dismiss}
					title="Dismiss digest and continue to dashboard"
				>
					Dismiss
				</button>
			</div>
		</div>
	</div>
{/if}
