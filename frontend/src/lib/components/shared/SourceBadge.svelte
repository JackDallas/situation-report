<script lang="ts">
	interface Props {
		sourceId: string;
		status?: 'healthy' | 'degraded' | 'down' | 'unknown';
	}

	let { sourceId, status = 'unknown' }: Props = $props();

	const statusColors = {
		healthy: 'bg-success/20 text-success border-success/30',
		degraded: 'bg-warning/20 text-warning border-warning/30',
		down: 'bg-alert/20 text-alert border-alert/30',
		unknown: 'bg-text-muted/20 text-text-muted border-text-muted/30',
	} as const;
</script>

<span class="inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs font-medium {statusColors[status]}" title="Source: {sourceId} — Status: {status}">
	{sourceId}
</span>
