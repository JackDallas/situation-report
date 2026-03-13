<script lang="ts">
	interface Props {
		lastUpdate: Date | null;
	}

	let { lastUpdate }: Props = $props();

	function formatAge(date: Date | null): string {
		if (!date) return 'No data';
		const seconds = Math.floor((Date.now() - date.getTime()) / 1000);
		if (seconds < 60) return `${seconds}s ago`;
		if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}h ago`;
		return `${Math.floor(seconds / 86400)}d ago`;
	}
</script>

<span class="text-xs text-text-muted" title="Time since last data update from this source">{formatAge(lastUpdate)}</span>
