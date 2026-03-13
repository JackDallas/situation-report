<script lang="ts">
	import { mapStore } from '$lib/stores/map.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';

	const RANGE_HOURS = 24;

	let dragging = $state(false);
	let sliderEl: HTMLDivElement;

	// The full range is last 24h
	const rangeMs = RANGE_HOURS * 60 * 60 * 1000;

	function getRangeStart(): number {
		return clockStore.now - rangeMs;
	}

	function toPercent(time: Date): number {
		const rangeStart = getRangeStart();
		const pct = ((time.getTime() - rangeStart) / rangeMs) * 100;
		return Math.max(0, Math.min(100, pct));
	}

	function fromPercent(pct: number): Date {
		const rangeStart = getRangeStart();
		return new Date(rangeStart + (pct / 100) * rangeMs);
	}

	function formatTime(date: Date): string {
		return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
	}

	function handlePointerDown(e: PointerEvent) {
		dragging = true;
		(e.target as HTMLElement).setPointerCapture(e.pointerId);
		updateCursorFromPointer(e);
	}

	function handlePointerMove(e: PointerEvent) {
		if (!dragging || !sliderEl) return;
		updateCursorFromPointer(e);
	}

	function updateCursorFromPointer(e: PointerEvent) {
		if (!sliderEl) return;
		const rect = sliderEl.getBoundingClientRect();
		const pct = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
		const time = fromPercent(pct);
		mapStore.setTimeCursor(time);
	}

	function handlePointerUp() {
		dragging = false;
	}

	function handleTrackClick(e: MouseEvent) {
		if (!sliderEl) return;
		const rect = sliderEl.getBoundingClientRect();
		const pct = Math.max(0, Math.min(100, ((e.clientX - rect.left) / rect.width) * 100));
		const time = fromPercent(pct);
		mapStore.setTimeCursor(time);
	}

	// Advance cursor in live mode
	$effect(() => {
		if (mapStore.isLive) {
			// clockStore.now ticks every 10s — keep cursor at real time
			mapStore.timeCursor = new Date(clockStore.now);
		}
	});

	let cursorPct = $derived(toPercent(mapStore.timeCursor));
	let rangeStartTime = $derived(new Date(getRangeStart()));
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex items-center gap-2 rounded-lg border border-border-default bg-bg-primary/90 px-3 py-1.5 backdrop-blur-sm">
	<!-- Range start label -->
	<span class="min-w-[36px] text-[10px] tabular-nums text-text-muted">
		{formatTime(rangeStartTime)}
	</span>

	<!-- Slider track -->
	<div
		class="relative h-4 flex-1 cursor-pointer"
		bind:this={sliderEl}
		onpointermove={handlePointerMove}
		onpointerup={handlePointerUp}
		onclick={handleTrackClick}
	>
		<!-- Background track -->
		<div class="absolute inset-y-1.5 left-0 right-0 rounded bg-white/10"></div>

		<!-- Past region (left of cursor) — subtle fill to show "visible" time -->
		<div
			class="absolute inset-y-1.5 left-0 rounded-l bg-accent/15"
			style="width: {cursorPct}%;"
		></div>

		<!-- Cursor handle -->
		<div
			class="absolute top-0 h-4 w-1 -translate-x-1/2 cursor-ew-resize rounded-sm bg-accent shadow-[0_0_4px_rgba(59,130,246,0.5)]"
			style="left: {cursorPct}%;"
			onpointerdown={handlePointerDown}
			role="slider"
			tabindex="0"
			aria-label="Time cursor"
			aria-valuemin={0}
			aria-valuemax={100}
			aria-valuenow={Math.round(cursorPct)}
		></div>

		<!-- Cursor time tooltip (visible while dragging or hovering) -->
		<div
			class="pointer-events-none absolute -top-5 -translate-x-1/2 rounded bg-bg-secondary px-1 text-[9px] tabular-nums text-text-secondary opacity-0 transition-opacity {dragging ? 'opacity-100' : 'group-hover:opacity-100'}"
			style="left: {cursorPct}%;"
		>
			{formatTime(mapStore.timeCursor)}
		</div>
	</div>

	<!-- Current cursor time label -->
	<span class="min-w-[36px] text-right text-[10px] tabular-nums text-text-muted">
		{formatTime(mapStore.timeCursor)}
	</span>

	<!-- LIVE button -->
	<button
		class="rounded px-2 py-0.5 text-[10px] font-bold transition-colors {mapStore.isLive
			? 'bg-red-500/20 text-red-400'
			: 'bg-white/5 text-text-muted hover:text-text-secondary'}"
		onclick={() => mapStore.goLive()}
		title={mapStore.isLive ? 'Live — showing events in real-time' : 'Click to snap to current time'}
	>
		{#if mapStore.isLive}
			<span class="mr-0.5 inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-red-500"></span>
		{/if}
		LIVE
	</button>
</div>
