import type { GeoJSONFeatureCollection, AnalysisReport, Summary, SituationCluster } from '$lib/types/events';
import type { SourceInfo } from '$lib/types/sources';
import type { PositionEntry } from '$lib/stores/map.svelte';

const BASE = '';

/** Entity from GET /api/entities */
export interface EntityNode {
	id: string;
	entity_type: string;
	canonical_name: string;
	aliases: string[];
	wikidata_id: string | null;
	properties: Record<string, unknown>;
	status: string;
	confidence: number;
	mention_count: number;
	first_seen_at: string;
	last_seen_at: string;
	last_enriched_at: string | null;
}

/** Relationship from entity detail */
export interface EntityRelationship {
	id: string;
	source_entity: string;
	target_entity: string;
	relationship: string;
	properties: Record<string, unknown>;
	confidence: number;
	evidence_count: number;
	first_seen_at: string;
	last_seen_at: string;
	is_active: boolean;
}

/** State change from entity detail */
export interface EntityStateChange {
	id: string;
	entity_id: string;
	change_type: string;
	previous_state: Record<string, unknown> | null;
	new_state: Record<string, unknown>;
	certainty: string;
	source_reliability: string;
	info_credibility: string;
	triggering_event_id: string | null;
	detected_at: string;
}

/** Full entity detail response from GET /api/entities/:id */
export interface EntityDetailResponse {
	entity: EntityNode;
	relationships: EntityRelationship[];
	state_changes: EntityStateChange[];
}

/** A single point in a position trail from GET /api/positions/:entity_id/trail */
export interface ApiTrailPoint {
	latitude: number;
	longitude: number;
	heading: number | null;
	speed: number | null;
	altitude: number | null;
	recorded_at: string;
}

/** Matches the backend sr_db::models::Event struct. */
export interface StoredEvent {
	event_time: string;
	ingested_at: string;
	source_type: string;
	source_id: string | null;
	latitude: number | null;
	longitude: number | null;
	region_code: string | null;
	entity_id: string | null;
	entity_name: string | null;
	event_type: string | null;
	severity: string | null;
	confidence: number | null;
	tags: string[] | null;
	title: string | null;
	description: string | null;
	payload: Record<string, unknown>;
}

/** Search result from GET /api/search */
export interface SearchResult {
	source_type: string;
	source_id: string | null;
	title: string | null;
	event_type: string | null;
	severity: string | null;
	event_time: string;
	region_code: string | null;
	score: number;
	match_type: string;
}

/** Similar event result from GET /api/search/similar */
export interface SimilarResult {
	source_type: string;
	source_id: string | null;
	title: string | null;
	event_type: string | null;
	event_time: string;
	distance: number;
}

/** Budget status from GET /api/intel/budget */
export interface BudgetStatus {
	spent_today_usd: number;
	daily_budget_usd: number;
	remaining_usd: number;
	budget_exhausted: boolean;
	degraded: boolean;
}

/** Pipeline metrics from GET /api/pipeline/metrics */
export interface PipelineMetrics {
	events_ingested: number;
	events_correlated: number;
	events_enriched: number;
	events_published: number;
	events_filtered: number;
	incidents_created: number;
	gpu_paused: boolean;
	gpu_state?: 'on' | 'starting' | 'off' | 'stopping';
}

/** Source health from GET /api/analytics/sources/health */
export interface SourceHealthEntry {
	source_id: string;
	status: string;
	last_success: string | null;
	last_error: string | null;
	consecutive_failures: number;
	total_events_24h: number;
}

async function fetchJson<T>(path: string, init?: RequestInit): Promise<T> {
	const res = await fetch(`${BASE}${path}`, init);
	if (!res.ok) {
		throw new Error(`API error: ${res.status} ${res.statusText}`);
	}
	return res.json();
}

export const api = {
	async getEvents(params?: {
		source?: string;
		event_type?: string;
		region?: string;
		since?: string;
		limit?: number;
		offset?: number;
		exclude?: string;
	}): Promise<StoredEvent[]> {
		const searchParams = new URLSearchParams();
		if (params?.source) searchParams.set('source', params.source);
		if (params?.event_type) searchParams.set('event_type', params.event_type);
		if (params?.region) searchParams.set('region', params.region);
		if (params?.since) searchParams.set('since', params.since);
		if (params?.limit) searchParams.set('limit', String(params.limit));
		if (params?.offset) searchParams.set('offset', String(params.offset));
		if (params?.exclude) searchParams.set('exclude', params.exclude);
		const qs = searchParams.toString();
		return fetchJson(`/api/events${qs ? `?${qs}` : ''}`);
	},

	async getLatestEvents(): Promise<StoredEvent[]> {
		return fetchJson('/api/events/latest');
	},

	async getEventsGeo(
		since?: string,
		limit?: number,
		types?: string,
		exclude?: string,
		bbox?: [number, number, number, number] | null,
		zoom?: number,
	): Promise<GeoJSONFeatureCollection> {
		const searchParams = new URLSearchParams();
		if (since) searchParams.set('since', since);
		if (limit) searchParams.set('limit', String(limit));
		if (types) searchParams.set('types', types);
		if (exclude) searchParams.set('exclude', exclude);
		if (bbox) {
			searchParams.set('min_lon', String(bbox[0]));
			searchParams.set('min_lat', String(bbox[1]));
			searchParams.set('max_lon', String(bbox[2]));
			searchParams.set('max_lat', String(bbox[3]));
		}
		if (zoom != null) searchParams.set('zoom', String(zoom));
		const qs = searchParams.toString();
		return fetchJson(`/api/events/geo${qs ? `?${qs}` : ''}`);
	},

	async getSources(): Promise<SourceInfo[]> {
		return fetchJson('/api/sources');
	},

	async toggleSource(sourceId: string): Promise<{ enabled: boolean }> {
		return fetchJson(`/api/sources/${sourceId}/toggle`, { method: 'POST' });
	},

	async updateSourceConfig(
		sourceId: string,
		config: { enabled: boolean; poll_interval_secs?: number; extra_config?: Record<string, unknown> },
	): Promise<void> {
		await fetchJson(`/api/sources/${sourceId}/config`, {
			method: 'PUT',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(config),
		});
	},

	async getStats(): Promise<{ total_events: number; events_24h: number }> {
		return fetchJson('/api/stats');
	},

	async getEntities(limit = 100): Promise<EntityNode[]> {
		return fetchJson(`/api/entities?limit=${limit}`);
	},

	async getEntityDetail(id: string): Promise<EntityDetailResponse> {
		return fetchJson(`/api/entities/${id}`);
	},

	async getPositionTrail(entityId: string, hours = 2, limit = 500): Promise<ApiTrailPoint[]> {
		const params = new URLSearchParams();
		params.set('hours', String(hours));
		params.set('limit', String(limit));
		return fetchJson(`/api/positions/${encodeURIComponent(entityId)}/trail?${params}`);
	},

	async getLatestAnalysis(): Promise<AnalysisReport | null> {
		return fetchJson('/api/intel/latest');
	},

	async getSummaries(): Promise<Summary[]> {
		return fetchJson('/api/pipeline/summaries');
	},

	async getSituations(): Promise<SituationCluster[]> {
		return fetchJson('/api/situations');
	},

	async getIncidents(params?: { limit?: number; since?: string }): Promise<any[]> {
		const searchParams = new URLSearchParams();
		if (params?.limit) searchParams.set('limit', String(params.limit));
		if (params?.since) searchParams.set('since', params.since);
		const qs = searchParams.toString();
		return fetchJson(`/api/incidents${qs ? `?${qs}` : ''}`);
	},

	async getSatelliteTles(): Promise<{ name: string; norad_id: number; tle_line1: string; tle_line2: string }[]> {
		return fetchJson('/api/satellite-tles');
	},

	async getPositions(params?: {
		bbox?: [number, number, number, number] | null;
		since?: string;
	}): Promise<PositionEntry[]> {
		const searchParams = new URLSearchParams();
		if (params?.bbox) {
			searchParams.set('min_lon', String(params.bbox[0]));
			searchParams.set('min_lat', String(params.bbox[1]));
			searchParams.set('max_lon', String(params.bbox[2]));
			searchParams.set('max_lat', String(params.bbox[3]));
		}
		if (params?.since) searchParams.set('since', params.since);
		const qs = searchParams.toString();
		return fetchJson(`/api/positions${qs ? `?${qs}` : ''}`);
	},

	/** Hybrid lexical+semantic event search. */
	async searchEvents(query: string, limit = 20): Promise<SearchResult[]> {
		const params = new URLSearchParams();
		params.set('q', query);
		params.set('limit', String(limit));
		return fetchJson(`/api/search?${params}`);
	},

	/** Find events similar to a given event (vector similarity). */
	async findSimilar(
		sourceType: string,
		sourceId: string,
		eventTime: string,
	): Promise<SimilarResult[]> {
		const params = new URLSearchParams();
		params.set('source_type', sourceType);
		params.set('source_id', sourceId);
		params.set('event_time', eventTime);
		return fetchJson(`/api/search/similar?${params}`);
	},

	/** Get AI budget status. */
	async getBudget(): Promise<BudgetStatus> {
		return fetchJson('/api/intel/budget');
	},

	/** Get pipeline metrics (throughput counters, GPU status). */
	async getPipelineMetrics(): Promise<PipelineMetrics> {
		return fetchJson('/api/pipeline/metrics');
	},

	/** Pause GPU — stops the llama container to free VRAM. */
	async pauseGpu(): Promise<{ gpu_state: string }> {
		return fetchJson('/api/pipeline/gpu/pause', { method: 'POST' });
	},

	/** Resume GPU — starts the llama container and waits for health. */
	async resumeGpu(): Promise<{ gpu_state: string }> {
		return fetchJson('/api/pipeline/gpu/resume', { method: 'POST' });
	},

	/** Get detailed source health from analytics. */
	async getSourcesHealth(): Promise<SourceHealthEntry[]> {
		return fetchJson('/api/analytics/sources/health');
	},
};
