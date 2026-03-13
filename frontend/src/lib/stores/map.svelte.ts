import { mapLayers } from '$lib/config/map-layers';
import type { MapLayerConfig } from '$lib/config/map-layers';
import type {
	GeoJSONFeatureCollection,
	GeoJSONFeature,
	SituationEvent,
	Incident
} from '$lib/types/events';
import { api } from '$lib/services/api';

export interface PositionEntry {
	entity_id: string;
	source_type: string;
	entity_name: string | null;
	latitude: number;
	longitude: number;
	heading: number | null;
	speed: number | null;
	altitude: number | null;
	last_seen: string;
	payload: Record<string, unknown>;
}

export interface TrailPoint {
	lng: number;
	lat: number;
	time: number;
}

class MapStore {
	layers = $state<MapLayerConfig[]>(structuredClone(mapLayers));
	geoData = $state<GeoJSONFeatureCollection>({ type: 'FeatureCollection', features: [] });
	center = $state<[number, number]>([44.0, 27.0]);
	zoom = $state(3);

	/** Version counter incremented whenever geoData or recentlyUpdated changes.
	 *  Used by MapPanel's batched updater to detect when a setData() call is needed. */
	geoVersion = $state(0);

	/** Current viewport bounds [west, south, east, north] */
	viewportBounds = $state<[number, number, number, number] | null>(null);

	/** Latest positions keyed by entity_id */
	positions = $state<Map<string, PositionEntry>>(new Map());

	/** Position trail history: entity_id -> last N points */
	positionHistory = $state<Map<string, TrailPoint[]>>(new Map());

	/** Lightweight entity metadata for trail styling — updated only on poll, not interpolation */
	entityMeta = $state<Map<string, { source_type: string; military: boolean }>>(new Map());

	private maxFeatures = 2000;
	private maxTrailPoints = 120;

	/** Recently updated source_ids with timestamps — drives pulse animation on map markers */
	recentlyUpdated = $state<Map<string, number>>(new Map());
	private static PULSE_DURATION_MS = 5000;

	/** Previous feature count per source_id for change detection */
	private prevSourceIdSet = new Set<string>();

	/** Payload cache keyed by source_id — payloads are stripped from GeoJSON features for performance */
	payloadCache = new Map<string, Record<string, unknown>>();

	/** Time cursor — the "current moment" on the timeline. Events before this are visible with decay. */
	timeCursor = $state<Date>(new Date());

	/** Whether the timeline cursor tracks real time */
	isLive = $state(true);

	/** Decay half-lives in minutes per event type (how quickly events fade after cursor passes them) */
	static DECAY_HALF_LIVES: Record<string, number> = {
		thermal_anomaly: 240,    // 4h — ephemeral detections
		conflict_event: 720,     // 12h
		news_article: 720,       // 12h
		geo_news: 720,           // 12h
		seismic_event: 1440,     // 24h
		nuclear_event: 2880,     // 48h
		gps_interference: 360,   // 6h
		internet_outage: 480,    // 8h
		notam_event: 1440,       // 24h — NOTAMs have explicit validity windows
		default: 720             // 12h fallback
	};

	setTimeCursor(time: Date) {
		this.timeCursor = time;
		this.isLive = false;
		this.geoVersion++;
	}

	goLive() {
		this.timeCursor = new Date();
		this.isLive = true;
		this.geoVersion++;
	}

	/** Get decay half-life in minutes for an event type */
	getDecayHalfLife(eventType: string): number {
		return MapStore.DECAY_HALF_LIVES[eventType] ?? MapStore.DECAY_HALF_LIVES['default'];
	}

	/** Whether the conflict heatmap overlay is visible */
	heatmapVisible = $state(true);

	toggleHeatmap() {
		this.heatmapVisible = !this.heatmapVisible;
	}

	/** Whether to hide ground planes (altitude <= 0) from position layer */
	hideGroundPlanes = $state(true);

	toggleGroundPlanes() {
		this.hideGroundPlanes = !this.hideGroundPlanes;
	}

	/** Whether confirmed strike / impact site markers are visible */
	impactSitesVisible = $state(true);

	toggleImpactSites() {
		this.impactSitesVisible = !this.impactSitesVisible;
	}

	/** Whether the military bases reference layer is visible */
	basesVisible = $state(false);

	toggleBases() {
		this.basesVisible = !this.basesVisible;
	}

	/** Whether AIS monitoring zone rectangles are visible */
	aisZonesVisible = $state(false);

	toggleAisZones() {
		this.aisZonesVisible = !this.aisZonesVisible;
	}

	/** Whether FIR boundary overlays are visible */
	firBoundariesVisible = $state(false);
	/** Whether FIR data has been loaded (lazy load on first toggle) */
	firBoundariesLoaded = $state(false);

	toggleFirBoundaries() {
		this.firBoundariesVisible = !this.firBoundariesVisible;
	}

	/** Whether restricted airspace overlays are visible */
	restrictedAirspaceVisible = $state(false);
	/** Whether restricted airspace data has been loaded (lazy load — 9.7MB) */
	restrictedAirspaceLoaded = $state(false);

	toggleRestrictedAirspace() {
		this.restrictedAirspaceVisible = !this.restrictedAirspaceVisible;
	}

	/** Whether NOTAM area overlays are visible */
	notamAreasVisible = $state(true);

	toggleNotamAreas() {
		this.notamAreasVisible = !this.notamAreasVisible;
	}

	/** Event types hidden from the map */
	hiddenEventTypes = $state<Set<string>>(new Set(['news_article', 'geo_news']));

	toggleEventType(type: string) {
		const next = new Set(this.hiddenEventTypes);
		if (next.has(type)) {
			next.delete(type);
		} else {
			next.add(type);
		}
		this.hiddenEventTypes = next;
	}

	showAllEventTypes() {
		this.hiddenEventTypes = new Set();
	}

	hideAllEventTypes() {
		this.hiddenEventTypes = new Set([
			'conflict_event', 'thermal_anomaly', 'seismic_event', 'nuclear_event',
			'notam_event', 'gps_interference', 'internet_outage', 'censorship_event',
			'news_article', 'geo_event', 'geo_news', 'telegram_message', 'threat_intel',
			'shodan_banner', 'fishing_event', 'bgp_leak'
		]);
	}

	toggleLayer(layerId: string) {
		const layer = this.layers.find((l) => l.id === layerId);
		if (layer) {
			layer.enabled = !layer.enabled;
		}
	}

	setView(center: [number, number], zoom: number) {
		this.center = center;
		this.zoom = zoom;
	}

	updateViewport(bounds: [number, number, number, number]) {
		this.viewportBounds = bounds;
	}

	/** Signal a fly-to request. MapPanel watches this and animates. */
	flyToTarget = $state<{ center: [number, number]; zoom: number } | null>(null);

	flyTo(lng: number, lat: number, zoom = 8) {
		this.flyToTarget = { center: [lng, lat], zoom };
	}

	updateGeoData(data: GeoJSONFeatureCollection) {
		// Detect new source_ids to trigger pulse on their map markers
		const now = Date.now();
		const nextSourceIdSet = new Set<string>();
		for (const f of data.features) {
			const sid = f.properties.source_id;
			if (sid) nextSourceIdSet.add(sid);
			// Cache payload separately and strip from feature properties
			if (sid && f.properties.payload) {
				const payload = f.properties.payload as Record<string, unknown>;
				this.payloadCache.set(sid, payload);
				// NOTAM: promote ICAO location code and radius for symbol/area layers
				if (f.properties.event_type === 'notam_event') {
					if (payload.location) {
						f.properties.notam_location = String(payload.location);
					}
					if (typeof payload.radius_nm === 'number' && payload.radius_nm > 0) {
						f.properties.notam_radius_nm = payload.radius_nm;
					}
				}
				delete f.properties.payload;
			}
		}

		// Any source_id present in new data but absent from previous data is "new"
		const nextUpdated = new Map(this.recentlyUpdated);
		for (const sid of nextSourceIdSet) {
			if (!this.prevSourceIdSet.has(sid)) {
				nextUpdated.set(sid, now);
			}
		}

		// Prune expired entries
		for (const [sid, ts] of nextUpdated) {
			if (now - ts > MapStore.PULSE_DURATION_MS) {
				nextUpdated.delete(sid);
			}
		}

		this.prevSourceIdSet = nextSourceIdSet;
		this.recentlyUpdated = nextUpdated;
		this.geoData = data;
		this.geoVersion++;
	}

	/** Update positions from /api/positions poll (merge — keeps existing entries) */
	updatePositions(entries: PositionEntry[]) {
		const nextPositions = new Map(this.positions);
		const nextHistory = new Map(this.positionHistory);
		const nextMeta = new Map(this.entityMeta);

		for (const entry of entries) {
			nextPositions.set(entry.entity_id, entry);
			this.appendTrail(nextHistory, entry);
			nextMeta.set(entry.entity_id, {
				source_type: entry.source_type,
				military: (entry.payload as any)?.military === true
			});
		}

		this.positions = nextPositions;
		this.positionHistory = nextHistory;
		this.entityMeta = nextMeta;
	}

	/** Replace all positions with fresh data (stale entries removed) */
	replacePositions(entries: PositionEntry[]) {
		const nextPositions = new Map<string, PositionEntry>();
		const nextHistory = new Map(this.positionHistory);
		const nextMeta = new Map<string, { source_type: string; military: boolean }>();

		for (const entry of entries) {
			nextPositions.set(entry.entity_id, entry);
			this.appendTrail(nextHistory, entry);
			nextMeta.set(entry.entity_id, {
				source_type: entry.source_type,
				military: (entry.payload as any)?.military === true
			});
		}

		// Remove trail history for entities no longer in the position set
		for (const entityId of nextHistory.keys()) {
			if (!nextPositions.has(entityId)) {
				nextHistory.delete(entityId);
			}
		}

		this.positions = nextPositions;
		this.positionHistory = nextHistory;
		this.entityMeta = nextMeta;
	}

	private appendTrail(history: Map<string, TrailPoint[]>, entry: PositionEntry) {
		const trail = history.get(entry.entity_id) ?? [];
		const newPoint: TrailPoint = {
			lng: entry.longitude,
			lat: entry.latitude,
			time: new Date(entry.last_seen).getTime()
		};
		const last = trail[0];
		if (!last || last.lng !== newPoint.lng || last.lat !== newPoint.lat) {
			history.set(
				entry.entity_id,
				[newPoint, ...trail].slice(0, this.maxTrailPoints)
			);
		}
	}

	/** Load full trail history for an entity from the backend API. */
	async loadEntityTrail(entityId: string, hours = 2): Promise<void> {
		try {
			const trail = await api.getPositionTrail(entityId, hours);
			const trailPoints: TrailPoint[] = trail.map((p) => ({
				lng: p.longitude,
				lat: p.latitude,
				time: new Date(p.recorded_at).getTime()
			}));
			const nextHistory = new Map(this.positionHistory);
			nextHistory.set(entityId, trailPoints.slice(0, this.maxTrailPoints));
			this.positionHistory = nextHistory;
		} catch (e) {
			console.warn(`Failed to load trail for ${entityId}:`, e);
		}
	}

	/** Append a single event to the map if it has coordinates. */
	addEventFeature(event: SituationEvent) {
		if (event.latitude == null || event.longitude == null) return;
		// Cache payload separately — omit from GeoJSON properties to reduce serialization cost
		if (event.source_id && event.payload) {
			this.payloadCache.set(event.source_id, event.payload);
		}
		const feature: GeoJSONFeature = {
			type: 'Feature',
			geometry: {
				type: 'Point',
				coordinates: [event.longitude, event.latitude]
			},
			properties: {
				source_type: event.source_type,
				source_id: event.source_id,
				event_type: event.event_type,
				event_time: event.event_time,
				entity_id: event.entity_id,
				entity_name: event.entity_name,
				severity: event.severity,
				confidence: event.confidence,
				title: event.title,
				region_code: event.region_code,
				// NOTAM: expose ICAO location code and radius for symbol/area layers
				...(event.event_type === 'notam_event' ? {
					...(event.payload?.location ? { notam_location: event.payload.location as string } : {}),
					...(typeof event.payload?.radius_nm === 'number' && (event.payload.radius_nm as number) > 0
						? { notam_radius_nm: event.payload.radius_nm as number } : {})
				} : {})
			}
		};
		// Mark as recently updated for pulse animation
		if (event.source_id) {
			const nextUpdated = new Map(this.recentlyUpdated);
			nextUpdated.set(event.source_id, Date.now());
			this.recentlyUpdated = nextUpdated;
			this.prevSourceIdSet.add(event.source_id);
		}
		const features = [feature, ...this.geoData.features].slice(0, this.maxFeatures);
		this.geoData = { type: 'FeatureCollection', features };
		this.geoVersion++;
	}

	/** Append an incident to the map if it has coordinates. */
	addIncidentFeature(incident: Incident) {
		if (incident.latitude == null || incident.longitude == null) return;
		const feature: GeoJSONFeature = {
			type: 'Feature',
			geometry: {
				type: 'Point',
				coordinates: [incident.longitude, incident.latitude]
			},
			properties: {
				source_type: 'pipeline',
				source_id: incident.rule_id,
				event_type: `incident:${incident.rule_id}`,
				event_time: incident.first_seen,
				entity_id: incident.id,
				entity_name: null,
				severity: incident.severity,
				confidence: incident.confidence,
				title: incident.title,
				region_code: incident.region_code
			}
		};
		// Mark incident rule_id as recently updated for pulse
		if (incident.rule_id) {
			const nextUpdated = new Map(this.recentlyUpdated);
			nextUpdated.set(incident.rule_id, Date.now());
			this.recentlyUpdated = nextUpdated;
			this.prevSourceIdSet.add(incident.rule_id);
		}
		const features = [feature, ...this.geoData.features].slice(0, this.maxFeatures);
		this.geoData = { type: 'FeatureCollection', features };
		this.geoVersion++;
	}
	/** Check whether a source_id is in the recently-updated set */
	isRecentlyUpdated(sourceId: string | null): boolean {
		if (!sourceId) return false;
		const ts = this.recentlyUpdated.get(sourceId);
		if (ts == null) return false;
		return Date.now() - ts <= MapStore.PULSE_DURATION_MS;
	}

	/** Prune stale entries from recentlyUpdated — called periodically from MapPanel */
	pruneRecentlyUpdated(): void {
		if (this.recentlyUpdated.size === 0) return; // fast path: nothing to prune
		const now = Date.now();
		// Check if anything actually needs pruning before allocating a new Map
		let needsPrune = false;
		for (const [, ts] of this.recentlyUpdated) {
			if (now - ts > MapStore.PULSE_DURATION_MS) {
				needsPrune = true;
				break;
			}
		}
		if (!needsPrune) return; // no expired entries — skip reactivity trigger
		const next = new Map<string, number>();
		for (const [sid, ts] of this.recentlyUpdated) {
			if (now - ts <= MapStore.PULSE_DURATION_MS) {
				next.set(sid, ts);
			}
		}
		this.recentlyUpdated = next;
		this.geoVersion++;
	}
}

export const mapStore = new MapStore();
