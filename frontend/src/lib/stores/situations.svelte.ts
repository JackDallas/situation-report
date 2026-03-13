import { eventStore } from '$lib/stores/events.svelte';
import {
	EVENT_TYPE_CATEGORY,
	REGION_LABELS,
	type Situation,
	type SituationCategory,
} from '$lib/types/situations';
import type { SituationCluster, SituationEvent } from '$lib/types/events';

const SEVERITY_RANK: Record<string, number> = { critical: 4, high: 3, medium: 2, low: 1 };
const SIX_HOURS_MS = 6 * 60 * 60 * 1000;

function severityRank(s: string): number {
	return SEVERITY_RANK[s] ?? 0;
}

const PHASE_BONUS: Record<string, number> = {
	active: 500,
	developing: 300,
	emerging: 100,
	declining: -200,
};

/** Composite score for ranking situations. Considers severity, source diversity,
 *  recency, phase, parent status, and log-scaled event count. */
function situationScore(s: Situation, now: number): number {
	let score = 0;

	// Severity is the primary signal
	score += severityRank(s.severity) * 10000;

	// Source diversity — multi-source situations are corroborated intelligence,
	// single-source are often sensor noise
	score += Math.min(s.sourceCount, 4) * 2000;

	// Recency — actively updated situations matter more
	const ageHours = (now - new Date(s.lastUpdated).getTime()) / 3600000;
	if (ageHours < 1) score += 1500;
	else if (ageHours < 3) score += 800;
	else if (ageHours < 6) score += 300;

	// Event count — LOG scale prevents noise magnets from dominating
	score += Math.round(Math.log2(Math.max(s.eventCount, 1)) * 30);

	// Phase: active/developing situations are most important
	score += PHASE_BONUS[s.phase ?? 'emerging'] ?? 0;

	// Parent situations aggregating sub-situations are more significant
	score += Math.min(s.childIds.length * 50, 500);

	return score;
}

function highestSeverity(events: SituationEvent[]): string {
	let best = 'low';
	for (const e of events) {
		if (severityRank(e.severity) > severityRank(best)) best = e.severity;
	}
	return best;
}

function categoryTitle(category: SituationCategory, region: string): string {
	const regionLabel = REGION_LABELS[region] ?? region;
	switch (category) {
		case 'conflict':
			return `Conflict & Military in ${regionLabel}`;
		case 'cyber':
			return `Cyber Activity in ${regionLabel}`;
		case 'environmental':
			return `Environmental Events in ${regionLabel}`;
		case 'intelligence':
			return `Intelligence in ${regionLabel}`;
		default:
			return `Activity in ${regionLabel}`;
	}
}

function centroid(events: SituationEvent[]): { lat: number | null; lng: number | null } {
	let sumLat = 0;
	let sumLng = 0;
	let count = 0;
	for (const e of events) {
		if (e.latitude != null && e.longitude != null) {
			sumLat += e.latitude;
			sumLng += e.longitude;
			count++;
		}
	}
	if (count === 0) return { lat: null, lng: null };
	return { lat: sumLat / count, lng: sumLng / count };
}


class SituationsStore {
	selectedSituation = $state<Situation | null>(null);
	backendClusters = $state<SituationCluster[]>([]);

	/** Tracks previous event counts per situation to detect updates */
	private prevEventCounts = new Map<string, number>();

	/** Tracks when each situation was last updated (event_count changed) */
	updatedAtMap = $state<Map<string, number>>(new Map());

	/** Lookup map for finding situations by ID (used by drawer navigation) */
	situationById = $derived.by(() => {
		const map = new Map<string, Situation>();
		for (const s of this.situations) {
			map.set(s.id, s);
		}
		return map;
	});

	situations = $derived.by(() => {
		const result: Situation[] = [];

		// Pipeline incidents are shown as AlertBanner toasts, not in the situation list.
		// The situation list shows only curated backend clusters.

		// 1. Backend entity-graph clusters → situations
		for (const cluster of this.backendClusters) {
			const region = cluster.region_codes[0] ?? 'global';
			// Determine category from source types
			const hasConflict = cluster.source_types.some((s) =>
				['acled', 'geoconfirmed'].includes(s)
			);
			const hasCyber = cluster.source_types.some((s) =>
				['cloudflare', 'ioda', 'bgp', 'otx', 'certstream', 'ooni', 'shodan'].includes(s)
			);
			const category: SituationCategory = hasConflict
				? 'conflict'
				: hasCyber
					? 'cyber'
					: 'intelligence';

			result.push({
				id: `cluster:${cluster.id}`,
				title: cluster.title,
				category,
				region,
				severity: cluster.severity,
				lastUpdated: cluster.last_updated,
				firstSeen: cluster.first_seen,
				sourceCount: cluster.source_count,
				sources: cluster.source_types,
				eventCount: cluster.event_count,
				events: [],
				incident: null,
				latitude: cluster.centroid?.[0] ?? null,
				longitude: cluster.centroid?.[1] ?? null,
				parentId: cluster.parent_id ? `cluster:${cluster.parent_id}` : null,
				childIds: (cluster.child_ids ?? []).map((id: string) => `cluster:${id}`),
				relatedIds: [],
				displayTitle: null,
				entities: cluster.entities,
				topics: cluster.topics,
				supplementary: cluster.supplementary ?? null,
				phase: cluster.phase ?? null,
				phaseChangedAt: cluster.phase_changed_at ?? null,
				peakEventRate: cluster.peak_event_rate ?? null,
				certainty: cluster.certainty ?? undefined,
			});
		}

		// 3. Fallback: cluster remaining events by region_code + category
		// Only if no backend clusters are available (graceful degradation)
		if (this.backendClusters.length === 0) {
			const now = Date.now();
			const cutoff = now - SIX_HOURS_MS;
			// Skip news_article (has dedicated News Feed panel) and geo_news (high-volume, map-only)
			const SKIP_CLUSTERING = new Set(['news_article', 'geo_news']);
			const clusters = new Map<string, SituationEvent[]>();
			for (const event of eventStore.events) {
				if (SKIP_CLUSTERING.has(event.event_type)) continue;
				const category = EVENT_TYPE_CATEGORY[event.event_type];
				if (!category) continue;

				try {
					if (new Date(event.event_time).getTime() < cutoff) continue;
				} catch {
					continue;
				}

				const region = event.region_code ?? 'global';
				const key = `${region}:${category}`;
				let cluster = clusters.get(key);
				if (!cluster) {
					cluster = [];
					clusters.set(key, cluster);
				}
				cluster.push(event);
			}

			for (const [key, events] of clusters) {
				if (events.length < 2) continue;

				const [region, category] = key.split(':') as [string, SituationCategory];
				const sources = [...new Set(events.map((e) => e.source_type))];
				const times = events
					.map((e) => new Date(e.event_time).getTime())
					.filter((t) => !isNaN(t));
				const earliest = times.length ? new Date(Math.min(...times)).toISOString() : '';
				const latest = times.length ? new Date(Math.max(...times)).toISOString() : '';
				const sev = highestSeverity(events);
				const c = centroid(events);

				result.push({
					id: key,
					title: categoryTitle(category, region),
					category,
					region,
					severity: sev,
					lastUpdated: latest,
					firstSeen: earliest,
					sourceCount: sources.length,
					sources,
					eventCount: events.length,
					events,
					incident: null,
					latitude: c.lat,
					longitude: c.lng,
					parentId: null,
					childIds: [],
					relatedIds: [],
					displayTitle: null,
				});
			}
		}

		// 4. Build parent/child hierarchy from incident linking + backend clusters
		const byId = new Map<string, Situation>();
		for (const s of result) {
			byId.set(s.id, s);
		}
		for (const s of result) {
			// Forward link: if this situation has a parentId, ensure parent knows about it
			if (s.parentId && byId.has(s.parentId)) {
				const parent = byId.get(s.parentId)!;
				if (!parent.childIds.includes(s.id)) {
					parent.childIds.push(s.id);
				}
			}
			// Reverse link: if this situation has childIds (from backend), ensure children know their parent
			for (const childId of s.childIds) {
				const child = byId.get(childId);
				if (child && !child.parentId) {
					child.parentId = s.id;
				}
			}
		}

		// 5. Sort: parents above children, then by composite score that
		// considers severity, source diversity, recency, phase, and event count.
		// This prevents single-source sensor noise from outranking real intel.
		const now = Date.now();
		result.sort((a, b) => {
			// Parents before children
			if (a.parentId && !b.parentId) return 1;
			if (!a.parentId && b.parentId) return -1;
			return situationScore(b, now) - situationScore(a, now);
		});

		return result;
	});

	/** Top-level situations only (no parent — used by AlertsPanel for tree rendering) */
	topLevel = $derived(this.situations.filter((s) => !s.parentId));

	/** Track event count changes and update timestamps accordingly */
	trackUpdates() {
		const now = Date.now();
		const nextMap = new Map(this.updatedAtMap);
		let changed = false;
		for (const s of this.situations) {
			const prev = this.prevEventCounts.get(s.id);
			if (prev !== undefined && s.eventCount !== prev) {
				nextMap.set(s.id, now);
				changed = true;
			}
			this.prevEventCounts.set(s.id, s.eventCount);
		}
		if (changed) {
			this.updatedAtMap = nextMap;
		}
	}
}

export const situationsStore = new SituationsStore();
