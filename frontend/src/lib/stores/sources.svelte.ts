import type { SourceInfo } from '$lib/types/sources';
import { api } from '$lib/services/api';

class SourceStore {
	sources = $state<SourceInfo[]>([]);
	loading = $state(false);
	private pollTimer: ReturnType<typeof setInterval> | null = null;

	async refresh() {
		this.loading = true;
		try {
			this.sources = await api.getSources();
		} catch (e) {
			console.error('Failed to fetch sources:', e);
		} finally {
			this.loading = false;
		}
	}

	/** Start polling sources every 30 seconds. Safe to call multiple times. */
	startPolling() {
		if (this.pollTimer) return;
		this.refresh();
		this.pollTimer = setInterval(() => this.refresh(), 30_000);
	}

	/** Stop polling. */
	stopPolling() {
		if (this.pollTimer) {
			clearInterval(this.pollTimer);
			this.pollTimer = null;
		}
	}

	async toggleSource(sourceId: string) {
		try {
			await api.toggleSource(sourceId);
			await this.refresh();
		} catch (e) {
			console.error('Failed to toggle source:', e);
		}
	}

	getSource(id: string): SourceInfo | undefined {
		return this.sources.find((s) => s.id === id);
	}
}

export const sourceStore = new SourceStore();
