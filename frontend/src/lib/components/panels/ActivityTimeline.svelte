<script lang="ts">
	interface Props {
		situationId: string;
	}

	let { situationId }: Props = $props();

	interface TimelineBucket {
		bucket: string;
		event_count: number;
		source_count: number;
		max_severity: string;
	}

	let buckets = $state<TimelineBucket[]>([]);
	let loading = $state(false);
	let error = $state(false);
	let hoveredIndex = $state<number | null>(null);

	const maxCount = $derived(Math.max(...buckets.map((b) => b.event_count), 1));

	function severityBarColor(severity: string): string {
		switch (severity) {
			case 'critical':
				return 'bg-red-500';
			case 'high':
				return 'bg-orange-500';
			case 'medium':
				return 'bg-yellow-500';
			case 'low':
				return 'bg-blue-500';
			default:
				return 'bg-gray-500';
		}
	}

	function severityBarOpacity(severity: string): string {
		switch (severity) {
			case 'critical':
				return 'opacity-90';
			case 'high':
				return 'opacity-80';
			case 'medium':
				return 'opacity-70';
			default:
				return 'opacity-60';
		}
	}

	function formatBucketTime(iso: string): string {
		const d = new Date(iso);
		return d.toLocaleTimeString('en-US', { hour: '2-digit', minute: '2-digit', hour12: false });
	}

	function formatBucketDate(iso: string): string {
		const d = new Date(iso);
		return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
	}

	$effect(() => {
		const id = situationId;
		if (!id) return;

		const clusterId = id.startsWith('cluster:') ? id.replace('cluster:', '') : id;
		loading = true;
		error = false;

		fetch(`/api/situations/${clusterId}/timeline`)
			.then((r) => {
				if (!r.ok) throw new Error('Failed to fetch timeline');
				return r.json();
			})
			.then((data: TimelineBucket[]) => {
				buckets = Array.isArray(data) ? data : [];
				loading = false;
			})
			.catch(() => {
				buckets = [];
				error = true;
				loading = false;
			});
	});
</script>

{#if loading}
	<div class="flex items-center gap-2 text-[10px] text-text-muted">
		<span class="h-3 w-3 animate-spin rounded-full border border-text-muted border-t-transparent"></span>
		Loading timeline...
	</div>
{:else if buckets.length > 0}
	<div class="relative">
		<!-- Bar chart -->
		<div class="flex items-end gap-px" style="height: 48px;">
			{#each buckets as bucket, i}
				{@const heightPct = bucket.event_count > 0 ? Math.max((bucket.event_count / maxCount) * 100, 8) : 0}
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div
					class="group relative flex-1"
					style="height: 100%;"
					onmouseenter={() => (hoveredIndex = i)}
					onmouseleave={() => (hoveredIndex = null)}
				>
					<!-- Bar -->
					<div
						class="absolute bottom-0 w-full rounded-t-sm transition-opacity {severityBarColor(bucket.max_severity)} {hoveredIndex === i ? 'opacity-100' : severityBarOpacity(bucket.max_severity)}"
						style="height: {heightPct}%;"
					></div>
				</div>
			{/each}
		</div>

		<!-- Baseline -->
		<div class="h-px w-full bg-border-default"></div>

		<!-- Time labels -->
		<div class="mt-1 flex justify-between text-[9px] text-text-muted">
			{#if buckets.length > 0}
				{@const first = buckets[0]}
				{#if first}
					<span>{formatBucketTime(first.bucket)}</span>
				{/if}
				{#if buckets.length > 1}
					{@const last = buckets[buckets.length - 1]}
					{#if last}
						<span>{formatBucketTime(last.bucket)}</span>
					{/if}
				{/if}
			{/if}
		</div>

		<!-- Tooltip -->
		{#if hoveredIndex !== null}
			{@const b = buckets[hoveredIndex]}
			{#if b}
				{@const leftPct = ((hoveredIndex + 0.5) / buckets.length) * 100}
				<div
					class="pointer-events-none absolute -top-14 z-10 whitespace-nowrap rounded border border-border-default bg-bg-primary px-2 py-1 text-[10px] shadow-lg"
					style="left: {leftPct}%; transform: translateX(-50%);"
				>
					<div class="font-medium text-text-primary">
						{b.event_count} event{b.event_count !== 1 ? 's' : ''}
					</div>
					<div class="text-text-muted">
						{b.source_count} source{b.source_count !== 1 ? 's' : ''} &middot; {formatBucketDate(b.bucket)} {formatBucketTime(b.bucket)}
					</div>
				</div>
			{/if}
		{/if}
	</div>
{:else if error}
	<p class="text-[10px] text-text-muted">Timeline unavailable</p>
{/if}
