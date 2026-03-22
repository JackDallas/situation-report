import { eventStore } from '$lib/stores/events.svelte';
import { mapStore } from '$lib/stores/map.svelte';
import { situationsStore } from '$lib/stores/situations.svelte';
import { api } from '$lib/services/api';
import {
	startInterpolation,
	stopInterpolation,
	setBasePositions,
	setMapInstance
} from '$lib/services/position-interpolator';
import type {
	SituationEvent,
	Incident,
	AnalysisReport,
	SituationCluster,
	EventType
} from '$lib/types/events';

/**
 * Event types that pass through the pipeline as individual events.
 * These are the editorially important / low-volume types that aren't absorbed.
 */
const PASSTHROUGH_EVENT_TYPES: readonly EventType[] = [
	'conflict_event',
	'thermal_anomaly',
	'news_article',
	'internet_outage',
	'threat_intel',
	'censorship_event',
	'geo_event',
	'seismic_event',
	'nuclear_event',
	'notam_event',
	'source_health',
	'telegram_message',
	'gps_interference',
	'fishing_event',
	'bgp_leak',
	'geo_news',
	'bluesky_post',
	'maritime_security'
];

const FEED_EXCLUDE =
	'bgp_anomaly,flight_position,vessel_position,cert_issued,shodan_banner,geo_news,shodan_count';
const INTEL_GEO_TYPES =
	'conflict_event,seismic_event,geo_event,nuclear_event,notam_event,internet_outage,gps_interference,censorship_event,threat_intel,fishing_event,geo_news,thermal_anomaly,bluesky_post,maritime_security,telegram_message,news_article';

/** All channels the client subscribes to */
const WS_CHANNELS = ['events', 'incidents', 'situations', 'alerts', 'analysis', 'summaries', 'source_health'];

let socket: WebSocket | null = null;
let summaryPollInterval: ReturnType<typeof setInterval> | null = null;
let positionPollInterval: ReturnType<typeof setInterval> | null = null;
let situationPollInterval: ReturnType<typeof setInterval> | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
let reconnectAttempts = 0;
const MAX_RECONNECT_DELAY = 16_000; // 16s max

function getReconnectDelay(): number {
	const base = Math.min(1000 * Math.pow(2, reconnectAttempts), MAX_RECONNECT_DELAY);
	// Add 0-25% jitter to avoid thundering herd
	const jitter = base * Math.random() * 0.25;
	reconnectAttempts++;
	return base + jitter;
}

function scheduleReconnect() {
	if (reconnectTimer) return; // already scheduled
	const delay = getReconnectDelay();
	console.log(`WebSocket reconnecting in ${Math.round(delay)}ms (attempt ${reconnectAttempts})`);
	eventStore.connectionStatus = 'reconnecting';
	reconnectTimer = setTimeout(() => {
		reconnectTimer = null;
		if (socket) {
			socket.close();
			socket = null;
		}
		openWS();
	}, delay);
}

/** Send a JSON message to the WebSocket if connected */
function sendWS(msg: Record<string, unknown>) {
	if (socket && socket.readyState === WebSocket.OPEN) {
		socket.send(JSON.stringify(msg));
	}
}

/** Send current viewport bounds to the server for geo_event filtering */
export function sendViewportToWS() {
	const bounds = mapStore.viewportBounds;
	if (bounds) {
		const [west, south, east, north] = bounds;
		sendWS({ type: 'viewport', bounds: { north, south, east, west } });
	}
}

/** Re-export setMapInstance so MapPanel can pass the map reference to the interpolator */
export { setMapInstance };

/** Throttle for viewport-based event re-fetch -- max once per 10 seconds */
let lastViewportFetchTime = 0;
let viewportFetchTimer: ReturnType<typeof setTimeout> | null = null;

async function loadInitialData() {
	try {
		// Only load events from last 12 hours to avoid stale map markers
		const geoSince = new Date(Date.now() - 12 * 60 * 60 * 1000).toISOString();
		// Pass viewport bounds for spatial filtering
		const bounds = mapStore.viewportBounds;
		const [events, intelGeo] = await Promise.all([
			api.getEvents({ limit: 200, exclude: FEED_EXCLUDE }),
			api.getEventsGeo(geoSince, 1500, INTEL_GEO_TYPES, undefined, bounds)
		]);
		if (events && events.length > 0) {
			const mapped = events.map((e) => ({
				event_time: e.event_time ?? e.ingested_at,
				source_type: e.source_type,
				source_id: e.source_id,
				latitude: e.latitude,
				longitude: e.longitude,
				region_code: e.region_code,
				entity_id: e.entity_id,
				entity_name: e.entity_name,
				event_type: e.event_type ?? 'unknown',
				severity: e.severity ?? 'low',
				confidence: e.confidence,
				tags: e.tags ?? [],
				title: e.title,
				description: e.description,
				payload: e.payload ?? {},
				heading: null,
				speed: null,
				altitude: null
			})) as SituationEvent[];
			eventStore.addEvents(mapped);
		}
		if (intelGeo?.features?.length) {
			mapStore.updateGeoData({
				type: 'FeatureCollection',
				features: intelGeo.features.slice(0, 1500)
			});
		}
		eventStore.connectionStatus = 'connected';
		lastViewportFetchTime = Date.now();
	} catch (err) {
		console.error('Failed to load initial data:', err);
	}
}

/**
 * Re-fetch geo events for a new viewport area (throttled to max once per 10s).
 * Called from MapPanel on significant viewport changes.
 */
export async function refetchGeoForViewport() {
	const now = Date.now();
	if (now - lastViewportFetchTime < 10_000) {
		// Schedule a deferred fetch if not already pending
		if (!viewportFetchTimer) {
			const delay = 10_000 - (now - lastViewportFetchTime);
			viewportFetchTimer = setTimeout(() => {
				viewportFetchTimer = null;
				refetchGeoForViewport();
			}, delay);
		}
		return;
	}
	lastViewportFetchTime = now;
	try {
		const geoSince = new Date(Date.now() - 12 * 60 * 60 * 1000).toISOString();
		const bounds = mapStore.viewportBounds;
		const intelGeo = await api.getEventsGeo(geoSince, 1500, INTEL_GEO_TYPES, undefined, bounds);
		if (intelGeo?.features?.length) {
			mapStore.updateGeoData({
				type: 'FeatureCollection',
				features: intelGeo.features.slice(0, 1500)
			});
		}
	} catch {
		// Silent -- viewport refetch is best-effort
	}
}

function openWS() {
	const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
	const url = `${proto}//${location.host}/api/ws`;
	socket = new WebSocket(url);

	socket.onopen = () => {
		eventStore.connectionStatus = 'connected';
		reconnectAttempts = 0; // reset backoff on successful connect
		// Subscribe to all channels
		sendWS({ type: 'subscribe', channels: WS_CHANNELS });
		// Send current viewport so server can push geo_events
		sendViewportToWS();
	};

	socket.onclose = () => {
		scheduleReconnect();
	};

	socket.onerror = () => {
		eventStore.connectionStatus = 'reconnecting';
	};

	socket.onmessage = (e: MessageEvent) => {
		try {
			const envelope = JSON.parse(e.data);
			const { type, data } = envelope;

			if (PASSTHROUGH_EVENT_TYPES.includes(type)) {
				// Individual event (editorially important / low-volume)
				if (data.kind === 'event') {
					const { kind: _, ...event } = data;
					const sitEvent = event as SituationEvent;
					eventStore.addEvent(sitEvent);
					mapStore.addEventFeature(sitEvent);
				}
			} else if (type.startsWith('incident:')) {
				// Cross-source correlated incident
				if (data.kind === 'incident') {
					const { kind: _, ...incident } = data;
					const inc = incident as Incident;
					// Normalize optional array fields the backend may omit via skip_serializing_if
					inc.related_ids ??= [];
					inc.merged_from ??= [];
					eventStore.addIncident(inc);
					mapStore.addIncidentFeature(inc);
				}
			} else if (type === 'analysis') {
				if (data.kind === 'analysis') {
					const { kind: _, ...report } = data;
					eventStore.updateAnalysis(report as AnalysisReport);
				}
			} else if (type === 'situations') {
				// Situation cluster updates
				let clusters: SituationCluster[] = [];
				if (Array.isArray(data)) {
					clusters = data;
				} else if (data.kind === 'situations' && Array.isArray(data.clusters)) {
					clusters = data.clusters;
				} else {
					// Try to find any array field
					for (const val of Object.values(data)) {
						if (Array.isArray(val)) {
							clusters = val as SituationCluster[];
							break;
						}
					}
				}
				// Normalize optional array fields the backend may omit
				for (const c of clusters) {
					c.event_titles ??= [];
				}
				situationsStore.backendClusters = clusters;
			}
			if (type === 'geo_event') {
				// Real-time geo event pushed by server for current viewport
				const sitEvent = data as SituationEvent;
				mapStore.addEventFeature(sitEvent);
			}
			// alert: and source_health: types are informational — no frontend handler needed yet
		} catch (err) {
			console.error('Failed to parse WebSocket message:', err);
		}
	};
}

export async function connectWS() {
	if (socket) {
		socket.close();
	}
	if (reconnectTimer) {
		clearTimeout(reconnectTimer);
		reconnectTimer = null;
	}
	reconnectAttempts = 0;

	await loadInitialData();
	openWS();

	// Load latest analysis on connect
	loadLatestAnalysis();

	// Poll summaries from REST endpoint (dashboard stats, not alert feed)
	// Slower fallback since WS now pushes updates
	pollSummaries();
	summaryPollInterval = setInterval(pollSummaries, 60_000);

	// Poll flight/vessel positions for live map tracking (these are absorbed, not on WS)
	pollPositions();
	positionPollInterval = setInterval(pollPositions, 60_000);

	// Poll backend situation clusters (fallback, WS pushes these too)
	loadSituations();
	situationPollInterval = setInterval(loadSituations, 60_000);

	// Load persisted incidents (fires before WS events arrive)
	loadIncidents();

	// Start dead reckoning interpolation for smooth position movement
	startInterpolation();
}

async function loadLatestAnalysis() {
	try {
		const report = await api.getLatestAnalysis();
		if (report) {
			eventStore.updateAnalysis(report);
		}
	} catch {
		// Silent -- analysis is supplementary
	}
}

async function pollSummaries() {
	try {
		const summaries = await api.getSummaries();
		for (const s of summaries) {
			eventStore.updateSummary(s);
		}
	} catch {
		// Silent -- summaries are supplementary
	}
}

async function loadSituations() {
	try {
		const clusters = await api.getSituations();
		if (Array.isArray(clusters)) {
			// Normalize optional array fields the backend may omit
			for (const c of clusters) {
				c.event_titles ??= [];
			}
			situationsStore.backendClusters = clusters;
		}
	} catch {
		// Silent -- situations are supplementary
	}
}

async function loadIncidents() {
	try {
		const incidents = await api.getIncidents({ limit: 50 });
		if (Array.isArray(incidents)) {
			for (const inc of incidents) {
				// Normalize optional array fields the backend may omit via skip_serializing_if
				inc.related_ids ??= [];
				inc.merged_from ??= [];
				eventStore.addIncident(inc);
				mapStore.addIncidentFeature(inc);
			}
		}
	} catch {
		// Silent -- incidents loaded via WS as they arrive
	}
}

async function pollPositions() {
	try {
		const positions = await api.getPositions({
			bbox: mapStore.viewportBounds,
			since: new Date(Date.now() - 10 * 60 * 1000).toISOString()
		});
		// Replace positions entirely so stale entries disappear
		mapStore.replacePositions(positions ?? []);
		setBasePositions(mapStore.positions);
	} catch {
		// Silent -- positions are supplementary
	}
}

export function disconnectWS() {
	stopInterpolation();
	if (reconnectTimer) {
		clearTimeout(reconnectTimer);
		reconnectTimer = null;
	}
	if (viewportFetchTimer) {
		clearTimeout(viewportFetchTimer);
		viewportFetchTimer = null;
	}
	reconnectAttempts = 0;
	if (socket) {
		socket.close();
		socket = null;
		eventStore.connectionStatus = 'disconnected';
	}
	if (summaryPollInterval) {
		clearInterval(summaryPollInterval);
		summaryPollInterval = null;
	}
	if (positionPollInterval) {
		clearInterval(positionPollInterval);
		positionPollInterval = null;
	}
	if (situationPollInterval) {
		clearInterval(situationPollInterval);
		situationPollInterval = null;
	}
}
