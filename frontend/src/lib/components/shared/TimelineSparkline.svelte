<script lang="ts">
	import { eventStore } from '$lib/stores/events.svelte';
	import { SEVERITY_RANK } from '$lib/config/colors';

	const WIDTH = 120;
	const HEIGHT = 24;
	const BUCKET_COUNT = 24; // 6h / 15min = 24 buckets
	const BUCKET_MS = 15 * 60 * 1000; // 15 minutes
	const WINDOW_MS = 6 * 60 * 60 * 1000; // 6 hours

	const buckets = $derived.by(() => {
		const now = Date.now();
		const windowStart = now - WINDOW_MS;
		const result: { count: number; maxSeverity: number }[] = Array.from(
			{ length: BUCKET_COUNT },
			() => ({ count: 0, maxSeverity: 0 })
		);

		for (const event of eventStore.events) {
			try {
				const ts = new Date(event.event_time).getTime();
				if (ts < windowStart || ts > now) continue;
				const idx = Math.min(
					Math.floor((ts - windowStart) / BUCKET_MS),
					BUCKET_COUNT - 1
				);
				const bucket = result[idx];
				if (!bucket) continue;
				bucket.count++;
				const sev = SEVERITY_RANK[event.severity] ?? 0;
				if (sev > bucket.maxSeverity) {
					bucket.maxSeverity = sev;
				}
			} catch {
				// skip
			}
		}

		return result;
	});

	const maxCount = $derived(Math.max(...buckets.map((b) => b.count), 1));

	const barWidth = WIDTH / BUCKET_COUNT - 1;

	function severityColor(sev: number): string {
		if (sev >= 4) return '#ef4444'; // critical - red
		if (sev >= 3) return '#f97316'; // high - orange
		if (sev >= 2) return '#eab308'; // medium - yellow
		return '#3b82f6'; // low - blue
	}
</script>

<svg
	width={WIDTH}
	height={HEIGHT}
	viewBox="0 0 {WIDTH} {HEIGHT}"
	class="shrink-0"
	role="img"
	aria-label="Event density over last 6 hours"
>
	{#each buckets as bucket, i}
		{@const barHeight = Math.max((bucket.count / maxCount) * (HEIGHT - 2), bucket.count > 0 ? 2 : 0)}
		<rect
			x={i * (barWidth + 1)}
			y={HEIGHT - barHeight}
			width={barWidth}
			height={barHeight}
			fill={bucket.count > 0 ? severityColor(bucket.maxSeverity) : 'transparent'}
			opacity={bucket.count > 0 ? 0.7 : 0}
			rx="1"
		/>
	{/each}
	<!-- baseline -->
	<line x1="0" y1={HEIGHT - 0.5} x2={WIDTH} y2={HEIGHT - 0.5} stroke="#374151" stroke-width="0.5" />
</svg>
