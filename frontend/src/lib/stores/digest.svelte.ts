import { api, type StoredEvent } from '$lib/services/api';
import { EVENT_TYPE_CATEGORY, type SituationCategory } from '$lib/types/situations';

const STORAGE_KEY = 'sr_last_visit';

export interface DigestData {
	lastVisit: string;
	totalEvents: number;
	totalIncidentLike: number;
	highestSeverity: string;
	breakdown: { category: SituationCategory; count: number }[];
	topEvents: { title: string; severity: string; event_type: string; event_time: string }[];
	timeSinceLabel: string;
}

class DigestStore {
	digest = $state<DigestData | null>(null);
	loading = $state(false);

	async load() {
		const lastVisit = localStorage.getItem(STORAGE_KEY);
		if (!lastVisit) {
			// First visit ever — set timestamp and skip digest
			this.saveTimestamp();
			return;
		}

		this.loading = true;
		try {
			const events = await api.getEvents({ since: lastVisit, limit: 500 });
			if (!events || events.length < 10) {
				// Not enough activity to show digest
				this.saveTimestamp();
				return;
			}

			const categoryCounts: Record<string, number> = {};
			let highestSev = 'low';
			let incidentLike = 0;

			const sevRank: Record<string, number> = { critical: 4, high: 3, medium: 2, low: 1 };

			for (const e of events) {
				const cat = EVENT_TYPE_CATEGORY[e.event_type ?? ''];
				if (cat) {
					categoryCounts[cat] = (categoryCounts[cat] || 0) + 1;
				}
				const sev = e.severity ?? 'low';
				if ((sevRank[sev] ?? 0) > (sevRank[highestSev] ?? 0)) {
					highestSev = sev;
				}
				if (sev === 'critical' || sev === 'high') {
					incidentLike++;
				}
			}

			const breakdown = Object.entries(categoryCounts)
				.map(([category, count]) => ({ category: category as SituationCategory, count }))
				.sort((a, b) => b.count - a.count);

			// Top events: highest severity first
			const topEvents = events
				.filter((e) => (sevRank[e.severity ?? 'low'] ?? 0) >= 2)
				.sort(
					(a, b) =>
						(sevRank[b.severity ?? 'low'] ?? 0) - (sevRank[a.severity ?? 'low'] ?? 0)
				)
				.slice(0, 3)
				.map((e) => ({
					title: e.title ?? summarizeEvent(e),
					severity: e.severity ?? 'low',
					event_type: e.event_type ?? 'unknown',
					event_time: e.event_time
				}));

			this.digest = {
				lastVisit,
				totalEvents: events.length,
				totalIncidentLike: incidentLike,
				highestSeverity: highestSev,
				breakdown,
				topEvents,
				timeSinceLabel: formatTimeSince(lastVisit)
			};
		} catch {
			// Silent — digest is supplementary
		} finally {
			this.loading = false;
		}
	}

	dismiss() {
		this.digest = null;
		this.saveTimestamp();
	}

	saveTimestamp() {
		localStorage.setItem(STORAGE_KEY, new Date().toISOString());
	}
}

function summarizeEvent(e: StoredEvent): string {
	if (e.title) return e.title;
	const p = e.payload ?? {};
	if (e.event_type === 'conflict_event')
		return `${p.event_type ?? 'Conflict'} in ${p.location ?? p.country ?? 'unknown'}`;
	if (e.event_type === 'seismic_event')
		return `M${p.magnitude ?? '?'} earthquake${p.place ? ` near ${p.place}` : ''}`;
	if (e.event_type === 'internet_outage')
		return `${p.severity ?? ''} outage in ${p.country ?? 'unknown'}`;
	return `${e.source_type}: ${e.event_type ?? 'event'}`;
}

function formatTimeSince(isoStr: string): string {
	const diffMs = Date.now() - new Date(isoStr).getTime();
	const mins = Math.floor(diffMs / 60000);
	if (mins < 60) return `${mins}m`;
	const hours = Math.floor(mins / 60);
	if (hours < 24) return `${hours}h`;
	const days = Math.floor(hours / 24);
	return `${days}d`;
}

export const digestStore = new DigestStore();
