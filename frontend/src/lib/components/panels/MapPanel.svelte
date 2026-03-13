<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { eventStore } from '$lib/stores/events.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import TimelineSlider from '$lib/components/shared/TimelineSlider.svelte';
	import { getOutlink, getEventDetails, escapeHtml } from '$lib/services/outlinks';
	import { typeColorMap, defaultColor, formatTimestamp, formatAbsoluteTime } from '$lib/services/event-display';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { setMapInstance } from '$lib/services/position-interpolator';
	import { refetchGeoForViewport } from '$lib/services/sse';
	import type { SituationEvent } from '$lib/types/events';

	const AFFILIATION_COLORS: Record<string, string> = {
		'US': '#3b82f6',    // blue
		'RU': '#ef4444',    // red
		'CN': '#f97316',    // orange
		'IL': '#22d3ee',    // cyan
		'IR': '#10b981',    // green
		'UA': '#eab308',    // yellow
		'GB': '#6366f1',    // indigo
		'FR': '#8b5cf6',    // violet
		'DE': '#ec4899',    // pink
		'NATO': '#818cf8',  // light indigo
	};
	const DEFAULT_MIL_COLOR = '#f472b6';   // pink (unknown military)
	const CIVILIAN_COLOR = '#64748b';       // slate
	const VESSEL_COLOR = '#06b6d4';         // cyan

	let container: HTMLDivElement;
	let map: any;
	let mapLoaded = $state(false);
	let pulseInterval: ReturnType<typeof setInterval> | null = null;

	/**
	 * Generate a GeoJSON Polygon approximating a circle on the Earth's surface.
	 * Uses the Haversine formula to compute points along the circumference.
	 * @param centerLng  Center longitude in degrees
	 * @param centerLat  Center latitude in degrees
	 * @param radiusNm   Radius in nautical miles
	 * @param numPoints  Number of polygon vertices (default 64)
	 */
	function circlePolygon(
		centerLng: number,
		centerLat: number,
		radiusNm: number,
		numPoints = 64
	): [number, number][] {
		const radiusKm = radiusNm * 1.852; // NM to km
		const R = 6371; // Earth radius in km
		const lat = (centerLat * Math.PI) / 180;
		const lng = (centerLng * Math.PI) / 180;
		const d = radiusKm / R; // angular distance in radians

		const coords: [number, number][] = [];
		for (let i = 0; i <= numPoints; i++) {
			const bearing = (2 * Math.PI * i) / numPoints;
			const pLat = Math.asin(
				Math.sin(lat) * Math.cos(d) +
				Math.cos(lat) * Math.sin(d) * Math.cos(bearing)
			);
			const pLng = lng + Math.atan2(
				Math.sin(bearing) * Math.sin(d) * Math.cos(lat),
				Math.cos(d) - Math.sin(lat) * Math.sin(pLat)
			);
			coords.push([(pLng * 180) / Math.PI, (pLat * 180) / Math.PI]);
		}
		return coords;
	}

	// --- Optimization #1: Batched event source updater ---
	let eventUpdateTimer: ReturnType<typeof setInterval> | null = null;
	/** Version counter — compared against mapStore.geoVersion to detect changes.
	 *  Also bumped by clock ticks for age_minutes refresh. */
	let lastRenderedGeoVersion = 0;
	let lastRenderedClockTick = 0;
	const CLUSTER_MAX_ZOOM = 7;

	/**
	 * Batched event source updater — runs on a 2s interval.
	 * Avoids the deep-clone JSON.parse(JSON.stringify()) by creating lightweight
	 * feature wrappers that share original geometry object references.
	 */
	function updateEventSource() {
		if (!mapLoaded || !map?.getSource('events') || !map?.getSource('events-unclustered')) return;

		// Skip if nothing has changed since last update (geoData/recentlyUpdated + clock tick)
		const storeVersion = mapStore.geoVersion;
		const clockTick = clockStore.now;
		if (storeVersion === lastRenderedGeoVersion && clockTick === lastRenderedClockTick) return;
		lastRenderedGeoVersion = storeVersion;
		lastRenderedClockTick = clockTick;

		const now = Date.now();
		const raw = mapStore.geoData;
		const cursorMs = mapStore.timeCursor.getTime();
		const recentlyUpdated = mapStore.recentlyUpdated;

		const features: any[] = [];
		const pulseFeatures: any[] = [];
		const notamAreaFeatures: any[] = [];

		for (const f of raw.features) {
			// Time cursor filter: only show events that occurred before the cursor.
			// FIRMS thermal_anomaly uses satellite pass times that can be hours in the future,
			// so allow a 4-hour grace window for those events.
			if (f.properties?.event_time && f.properties.event_type !== 'geo_event') {
				try {
					const t = new Date(f.properties.event_time).getTime();
					const grace = f.properties.event_type === 'thermal_anomaly' ? 4 * 60 * 60 * 1000 : 0;
					if (t > cursorMs + grace) continue; // future of cursor — hidden
				} catch { /* pass */ }
			}

			// Compute age_minutes from cursor position (not wall clock)
			// This drives decay-based opacity in layer paint expressions
			let ageMinutes = 0;
			if (f.properties?.event_time) {
				try {
					ageMinutes = Math.max(0, Math.floor(
						(cursorMs - new Date(f.properties.event_time).getTime()) / 60000
					));
				} catch { /* 0 */ }
			}

			// Drop events that would be fully invisible — fixes cluster counts including
			// phantom events (e.g. thermal_anomaly opacity→0 at 360min)
			if (f.properties?.event_type === 'thermal_anomaly' && ageMinutes >= 360) continue;

			const sid = f.properties?.source_id;
			const pulseTs = sid ? recentlyUpdated.get(sid) : undefined;
			const isRecent = pulseTs != null && (now - pulseTs <= 5000);

			// Create a lightweight wrapper sharing the original geometry reference
			const enrichedProps = {
				...f.properties,
				age_minutes: ageMinutes,
				recently_updated: isRecent
			};

			features.push({
				type: 'Feature',
				geometry: f.geometry,
				properties: enrichedProps
			});

			// Build NOTAM area polygons for events with radius data
			const notamRadius = f.properties?.notam_radius_nm;
			if (f.properties?.event_type === 'notam_event'
				&& typeof notamRadius === 'number' && notamRadius > 0
				&& f.geometry?.type === 'Point'
				&& f.geometry.coordinates
			) {
				const [lng, lat] = f.geometry.coordinates;
				const ring = circlePolygon(lng, lat, notamRadius);
				notamAreaFeatures.push({
					type: 'Feature',
					geometry: {
						type: 'Polygon',
						coordinates: [ring]
					},
					properties: {
						source_id: sid,
						event_type: 'notam_event',
						age_minutes: ageMinutes,
						title: f.properties.title,
						notam_location: f.properties.notam_location,
						severity: f.properties.severity,
						event_time: f.properties.event_time,
						entity_name: f.properties.entity_name,
						source_type: f.properties.source_type,
						region_code: f.properties.region_code,
						// Store center for popup positioning
						center_lng: lng,
						center_lat: lat,
						radius_nm: notamRadius
					}
				});
			}

			// Build pulse ring features for recently-updated markers
			if (isRecent && f.geometry?.coordinates) {
				const elapsed = now - pulseTs;
				const progress = Math.min(elapsed / 5000, 1);
				pulseFeatures.push({
					type: 'Feature',
					geometry: f.geometry,
					properties: {
						pulse_radius: 8 + progress * 20,
						pulse_opacity: Math.max(0, 1 - progress)
					}
				});
			}
		}

		const featureCollection = { type: 'FeatureCollection', features };

		// Update the clustered events source (used by cluster layers + circle layers)
		const eventsSource = map.getSource('events') as any;
		eventsSource.setData(featureCollection);

		// Update the unclustered source (used by heatmap which needs raw points)
		(map.getSource('events-unclustered') as any).setData(featureCollection);

		// Update pulse ring source
		if (map.getSource('pulse')) {
			(map.getSource('pulse') as any).setData({
				type: 'FeatureCollection',
				features: pulseFeatures
			});
		}

		// Update NOTAM area polygons source
		if (map.getSource('notam-areas')) {
			(map.getSource('notam-areas') as any).setData({
				type: 'FeatureCollection',
				features: notamAreaFeatures
			});
		}
	}

	/** Build popup HTML from feature properties. */
	function buildPopupHtml(props: Record<string, any>): string {
		// Look up payload from the separate cache (stripped from GeoJSON for performance)
		let payload: Record<string, unknown> = {};
		const sourceId = props.source_id;
		if (sourceId) {
			payload = mapStore.payloadCache.get(sourceId) ?? {};
		}
		// Fallback: if payload was still on the feature properties (e.g. from older data)
		if (Object.keys(payload).length === 0 && props.payload) {
			try {
				payload =
					typeof props.payload === 'string'
						? JSON.parse(props.payload)
						: (props.payload ?? {});
			} catch {
				payload = {};
			}
		}

		const pseudoEvent: SituationEvent = {
			event_time: props.event_time ?? '',
			source_type: props.source_type ?? '',
			source_id: props.source_id ?? null,
			latitude: null,
			longitude: null,
			region_code: props.region_code ?? null,
			entity_id: props.entity_id ?? null,
			entity_name: props.entity_name ?? null,
			event_type: props.event_type ?? '',
			severity: props.severity ?? 'low',
			confidence: props.confidence ?? null,
			tags: [],
			title: props.title ?? null,
			description: null,
			payload,
			heading: null,
			speed: null,
			altitude: null
		};

		const isIncident = pseudoEvent.event_type?.startsWith('incident:');
		const label = isIncident
			? pseudoEvent.event_type.replace('incident:', '').replace(/_/g, ' ')
			: pseudoEvent.event_type?.replace(/_/g, ' ');
		const typeLabel = typeColorMap[pseudoEvent.event_type]?.label ?? label;

		const time = pseudoEvent.event_time
			? `${formatAbsoluteTime(pseudoEvent.event_time)} (${formatTimestamp(pseudoEvent.event_time)})`
			: '';

		const details = getEventDetails(pseudoEvent);
		const outlink = getOutlink(pseudoEvent);

		const detailRows = details
			.slice(0, 4)
			.map(
				(f) =>
					`<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">${escapeHtml(f.label)}</span><span style="color:#d1d5db;">${escapeHtml(f.value)}</span></div>`
			)
			.join('');

		const outlinkHtml = outlink
			? `<a href="${escapeHtml(outlink.url)}" target="_blank" rel="noopener noreferrer" style="display:inline-flex;align-items:center;gap:4px;margin-top:6px;padding:3px 8px;background:rgba(59,130,246,0.15);color:#60a5fa;border-radius:4px;font-size:11px;text-decoration:none;font-weight:500;">${escapeHtml(outlink.label)} <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14"/></svg></a>`
			: '';

		const detailIdx = ((window as any).__srDetailProps ??= []).length;
		(window as any).__srDetailProps.push(props);
		const detailsBtn = `<button onclick="window.__srOpenDetail(${detailIdx})" style="margin-top:6px;margin-left:${outlink ? '6px' : '0'};padding:3px 8px;background:rgba(255,255,255,0.08);color:#9ca3af;border:1px solid rgba(255,255,255,0.1);border-radius:4px;font-size:11px;cursor:pointer;font-family:monospace;">Details</button>`;

		const severityBadge =
			pseudoEvent.severity && pseudoEvent.severity !== 'low'
				? `<span style="font-size:10px;padding:1px 6px;border-radius:3px;background:${pseudoEvent.severity === 'critical' ? 'rgba(239,68,68,0.2)' : pseudoEvent.severity === 'high' ? 'rgba(249,115,22,0.2)' : 'rgba(234,179,8,0.2)'};color:${pseudoEvent.severity === 'critical' ? '#f87171' : pseudoEvent.severity === 'high' ? '#fb923c' : '#facc15'};">${escapeHtml(pseudoEvent.severity.toUpperCase())}</span>`
				: '';

		// NOTAM decode enrichment for map popup
		let notamDecodeHtml = '';
		if (pseudoEvent.event_type === 'notam_event' && payload.qcode_description) {
			const isRoutine = payload.is_routine === true;
			const routineBadge = isRoutine
				? `<span style="font-size:10px;padding:1px 6px;border-radius:3px;background:rgba(16,185,129,0.15);color:#34d399;">Routine</span>`
				: `<span style="font-size:10px;padding:1px 6px;border-radius:3px;background:rgba(245,158,11,0.15);color:#fbbf24;">Significant</span>`;
			notamDecodeHtml = `<div style="margin-bottom:6px;padding:4px 6px;background:rgba(255,255,255,0.04);border-radius:4px;border-left:2px solid ${isRoutine ? '#34d399' : '#fbbf24'};">
				<div style="display:flex;align-items:center;gap:4px;margin-bottom:2px;">
					<span style="font-size:11px;font-weight:bold;color:#d1d5db;">${escapeHtml(String(payload.qcode_category))}</span>
					${routineBadge}
				</div>
				<div style="font-size:11px;color:#9ca3af;">${escapeHtml(String(payload.qcode_description))}</div>
			</div>`;
		}

		return `<div style="font-family:monospace;font-size:12px;color:#e5e7eb;max-width:280px;">
			<div style="display:flex;align-items:center;gap:6px;margin-bottom:6px;">
				<span style="font-weight:bold;color:${isIncident ? '#ef4444' : '#60a5fa'};">${isIncident ? 'INCIDENT' : escapeHtml(typeLabel)}</span>
				${severityBadge}
			</div>
			${notamDecodeHtml}
			${detailRows ? `<div style="margin-bottom:6px;display:flex;flex-direction:column;gap:2px;">${detailRows}</div>` : ''}
			<div style="color:#6b7280;font-size:11px;">${escapeHtml(time)}</div>
			<div style="display:flex;align-items:center;flex-wrap:wrap;">${outlinkHtml}${detailsBtn}</div>
		</div>`;
	}

	/** Create a rotated arrow/triangle icon for directional markers */
	function createArrowImage(
		mapInstance: any,
		id: string,
		color: string,
		size: number
	) {
		const canvas = document.createElement('canvas');
		canvas.width = size;
		canvas.height = size;
		const ctx = canvas.getContext('2d')!;
		const cx = size / 2;
		const cy = size / 2;
		const r = size / 2 - 2;

		// Arrow pointing up (heading=0 means north)
		ctx.beginPath();
		ctx.moveTo(cx, cy - r); // tip
		ctx.lineTo(cx - r * 0.6, cy + r * 0.7);
		ctx.lineTo(cx, cy + r * 0.3);
		ctx.lineTo(cx + r * 0.6, cy + r * 0.7);
		ctx.closePath();
		ctx.fillStyle = color;
		ctx.fill();
		ctx.strokeStyle = 'rgba(255,255,255,0.6)';
		ctx.lineWidth = 1;
		ctx.stroke();

		mapInstance.addImage(id, { width: size, height: size, data: ctx.getImageData(0, 0, size, size).data });
	}

	function updateViewportBounds() {
		if (!map) return;
		const bounds = map.getBounds();
		mapStore.updateViewport([
			bounds.getWest(),
			bounds.getSouth(),
			bounds.getEast(),
			bounds.getNorth()
		]);
	}

	onMount(async () => {
		const maplibre = await import('maplibre-gl');
		await import('maplibre-gl/dist/maplibre-gl.css');

		// Register global bridge for popup -> drawer communication
		(window as any).__srDetailProps = [];
		(window as any).__srOpenDetail = (idx: number) => {
			const props = (window as any).__srDetailProps?.[idx];
			if (!props) return;

			const found = props.source_id
				? eventStore.events.find(
						(e) => e.source_id === props.source_id && e.source_type === props.source_type
					)
				: null;

			if (found) {
				eventStore.selectedEvent = found;
				return;
			}

			// Look up payload from cache (stripped from GeoJSON features)
			let payload: Record<string, unknown> = {};
			if (props.source_id) {
				payload = mapStore.payloadCache.get(props.source_id) ?? {};
			}
			if (Object.keys(payload).length === 0 && props.payload) {
				try {
					payload =
						typeof props.payload === 'string'
							? JSON.parse(props.payload)
							: (props.payload ?? {});
				} catch {
					payload = {};
				}
			}
			const pseudoEvent: SituationEvent = {
				event_time: props.event_time ?? '',
				source_type: props.source_type ?? '',
				source_id: props.source_id ?? null,
				latitude: props.latitude ?? null,
				longitude: props.longitude ?? null,
				region_code: props.region_code ?? null,
				entity_id: props.entity_id ?? null,
				entity_name: props.entity_name ?? null,
				event_type: props.event_type ?? '',
				severity: props.severity ?? 'low',
				confidence: props.confidence ?? null,
				tags: [],
				title: props.title ?? null,
				description: props.description ?? null,
				payload,
				heading: null,
				speed: null,
				altitude: null
			};
			eventStore.selectedEvent = pseudoEvent;
		};

		map = new maplibre.Map({
			container,
			style: 'https://basemaps.cartocdn.com/gl/dark-matter-gl-style/style.json',
			center: mapStore.center,
			zoom: mapStore.zoom
		});

		map.on('load', () => {
			// Create arrow images for each affiliation + defaults
			for (const [code, color] of Object.entries(AFFILIATION_COLORS)) {
				createArrowImage(map, `arrow-mil-${code}`, color, 24);
			}
			createArrowImage(map, 'arrow-flight-mil', DEFAULT_MIL_COLOR, 24);
			createArrowImage(map, 'arrow-flight', CIVILIAN_COLOR, 24);
			createArrowImage(map, 'arrow-vessel', VESSEL_COLOR, 20);

			// --- Event layers ---
			// Optimization #3: Clustered events source for circle/symbol layers
			map.addSource('events', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] },
				cluster: true,
				clusterMaxZoom: 7,
				clusterRadius: 40,
				clusterProperties: {
					// Aggregate max severity rank for cluster styling
					maxSeverityRank: ['max', [
						'match', ['get', 'severity'],
						'critical', 4,
						'high', 3,
						'medium', 2,
						1
					]]
				}
			});

			// Unclustered events source — used by heatmap (needs raw points, not cluster centroids)
			map.addSource('events-unclustered', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// --- Pulse layer: expanding/fading rings on recently-updated markers ---
			map.addSource('pulse', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			map.addLayer({
				id: 'pulse-ring',
				type: 'circle',
				source: 'pulse',
				paint: {
					'circle-radius': ['get', 'pulse_radius'],
					'circle-color': 'transparent',
					'circle-stroke-width': 2,
					'circle-stroke-color': '#ffaa00',
					'circle-stroke-opacity': ['get', 'pulse_opacity']
				}
			});

			// --- NOTAM area polygons source — geographic circles for airspace restrictions ---
			map.addSource('notam-areas', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// NOTAM area fill — styled like restricted airspace zones
			map.addLayer({
				id: 'notam-area-fill',
				type: 'fill',
				source: 'notam-areas',
				paint: {
					'fill-color': [
						'match', ['get', 'severity'],
						'critical', 'rgba(239, 68, 68, 0.10)',
						'high', 'rgba(249, 115, 22, 0.08)',
						'medium', 'rgba(251, 146, 60, 0.06)',
						'rgba(251, 146, 60, 0.04)'
					],
					'fill-opacity': [
						'interpolate', ['linear'], ['zoom'],
						3, 0.4,
						6, 0.6,
						10, 0.8
					]
				}
			});

			// NOTAM area outline — dashed border like FIR boundaries
			map.addLayer({
				id: 'notam-area-outline',
				type: 'line',
				source: 'notam-areas',
				paint: {
					'line-color': [
						'match', ['get', 'severity'],
						'critical', '#ef4444',
						'high', '#f97316',
						'#fb923c'
					],
					'line-width': [
						'interpolate', ['linear'], ['zoom'],
						3, 0.5,
						8, 1,
						12, 1.5
					],
					'line-opacity': 0.5,
					'line-dasharray': [4, 3]
				}
			});

			// --- Cluster circle layer (Optimization #3) ---
			map.addLayer({
				id: 'event-clusters',
				type: 'circle',
				source: 'events',
				filter: ['has', 'point_count'],
				paint: {
					'circle-radius': [
						'step', ['get', 'point_count'],
						12,      // default radius for count < 10
						10, 16,  // radius for count 10-50
						50, 20,  // radius for count 50-100
						100, 24  // radius for count 100+
					],
					'circle-color': [
						'step', ['get', 'maxSeverityRank'],
						'#3b82f6',     // low (default blue)
						2, '#eab308',  // medium (yellow)
						3, '#f97316',  // high (orange)
						4, '#ef4444'   // critical (red)
					],
					'circle-opacity': 0.7,
					'circle-stroke-width': 2,
					'circle-stroke-color': 'rgba(255,255,255,0.3)'
				}
			});

			// Cluster count label
			map.addLayer({
				id: 'event-cluster-count',
				type: 'symbol',
				source: 'events',
				filter: ['has', 'point_count'],
				layout: {
					'text-field': ['concat', ['get', 'point_count_abbreviated'], '+'],
					'text-size': 11,
					'text-font': ['Open Sans Bold']
				},
				paint: {
					'text-color': '#ffffff'
				}
			});

			// Incident glow layer — only unclustered points
			map.addLayer({
				id: 'incidents-glow',
				type: 'circle',
				minzoom: CLUSTER_MAX_ZOOM,
				source: 'events',
				filter: ['all',
					['!', ['has', 'point_count']],
					['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:']
				],
				paint: {
					'circle-radius': 14,
					'circle-color': '#ef4444',
					'circle-opacity': 0.25,
					'circle-stroke-width': 0
				}
			});

			// Main event circles — only unclustered points (Optimization #3: filter out clusters)
			// Excludes thermal_anomaly (handled by dedicated thermal-dots layer)
			// minzoom matches clusterMaxZoom so dots only appear once clustering stops
			map.addLayer({
				id: 'events-circle',
				type: 'circle',
				source: 'events',
				minzoom: CLUSTER_MAX_ZOOM,
				filter: ['all',
					['!', ['has', 'point_count']],
					['!=', ['get', 'event_type'], 'thermal_anomaly']
				],
				paint: {
					'circle-radius': [
						'case',
						['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
						8,
						['match', ['get', 'severity'], 'critical', 7, 'high', 6, 5]
					],
					'circle-color': [
						'match',
						['get', 'event_type'],
						'conflict_event', '#ef4444',
						'thermal_anomaly', '#f97316',
						'seismic_event', '#eab308',
						'nuclear_event', '#f43f5e',
						'notam_event', '#fb923c',
						'gps_interference', '#d946ef',
						'internet_outage', '#a855f7',
						'censorship_event', '#8b5cf6',
						'news_article', '#22d3ee',
						'geo_event', '#14b8a6',
						'telegram_message', '#38bdf8',
						'threat_intel', '#f472b6',
						'shodan_banner', '#0ea5e9',
						'fishing_event', '#10b981',
						'bgp_leak', '#6366f1',
						'geo_news', '#34d399',
						[
							'case',
							['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
							'#ff0000',
							'#6b7280'
						]
					],
					'circle-opacity': [
						'case',
						['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
						1.0,
						// Age-based fading: >6h = 25%, >2h = 50%, else 80%
						[
							'interpolate',
							['linear'],
							['coalesce', ['get', 'age_minutes'], 0],
							0, 0.8,
							120, 0.5,
							360, 0.25
						]
					],
					'circle-stroke-width': [
						'case',
						['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
						2,
						['==', ['get', 'recently_updated'], true],
						3,
						1
					],
					'circle-stroke-color': [
						'case',
						['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
						'#ffffff',
						['==', ['get', 'recently_updated'], true],
						'#ffaa00',
						'rgba(255, 255, 255, 0.3)'
					]
				}
			});

			// --- Impact sites layer — GeoConfirmed confirmed events (subtle markers) ---
			// Only unclustered points; minzoom matches clusterMaxZoom
			map.addLayer({
				id: 'impact-sites',
				type: 'circle',
				source: 'events',
				minzoom: CLUSTER_MAX_ZOOM,
				filter: ['all',
					['!', ['has', 'point_count']],
					['==', ['get', 'event_type'], 'geo_event']
				],
				paint: {
					'circle-radius': [
						'match', ['get', 'severity'],
						'critical', 5,
						'high', 4.5,
						'medium', 4,
						3.5
					],
					'circle-color': '#14b8a6',
					'circle-opacity': [
						'interpolate', ['linear'],
						['coalesce', ['get', 'age_minutes'], 0],
						0, 0.8,
						360, 0.5,
						1440, 0.3
					],
					'circle-stroke-width': 1,
					'circle-stroke-color': 'rgba(20, 184, 166, 0.5)'
				}
			});

			// --- NOTAM center dot markers — fixed-size orange dot for each NOTAM location ---
			// Rendered as a fixed 6px circle; minzoom matches clusterMaxZoom.
			// NOTAMs with radius_nm also get area polygons from the 'notam-areas' source.
			map.addLayer({
				id: 'notam-zones',
				type: 'circle',
				source: 'events',
				minzoom: CLUSTER_MAX_ZOOM,
				filter: ['all',
					['!', ['has', 'point_count']],
					['==', ['get', 'event_type'], 'notam_event']
				],
				paint: {
					'circle-radius': 6,
					'circle-color': 'rgba(251, 146, 60, 0.25)',
					'circle-stroke-width': 1.5,
					'circle-stroke-color': '#fb923c',
					'circle-stroke-opacity': [
						'interpolate', ['linear'],
						['coalesce', ['get', 'age_minutes'], 0],
						0, 0.8,
						360, 0.5,
						1440, 0.3
					],
					'circle-opacity': [
						'interpolate', ['linear'],
						['coalesce', ['get', 'age_minutes'], 0],
						0, 0.8,
						360, 0.5,
						1440, 0.3
					]
				}
			});

			// NOTAM airport code labels — only unclustered points
			map.addLayer({
				id: 'notam-labels',
				type: 'symbol',
				source: 'events',
				minzoom: CLUSTER_MAX_ZOOM,
				filter: ['all',
					['!', ['has', 'point_count']],
					['==', ['get', 'event_type'], 'notam_event']
				],
				layout: {
					'text-field': ['coalesce', ['get', 'notam_location'], ''],
					'text-size': 9,
					'text-offset': [0, -1.8],
					'text-anchor': 'bottom',
					'text-allow-overlap': false
				},
				paint: {
					'text-color': '#fb923c',
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1
				}
			});

			// --- Thermal anomaly layer — tiny orange dots for FIRMS fire detections ---
			// Only unclustered points; minzoom matches clusterMaxZoom
			map.addLayer({
				id: 'thermal-dots',
				type: 'circle',
				minzoom: CLUSTER_MAX_ZOOM,
				source: 'events',
				filter: ['all',
					['!', ['has', 'point_count']],
					['==', ['get', 'event_type'], 'thermal_anomaly']
				],
				paint: {
					'circle-radius': [
						'interpolate', ['linear'], ['zoom'],
						3, 2,
						6, 3,
						10, 4,
						14, 6
					],
					'circle-color': '#f97316',
					'circle-opacity': [
						'interpolate', ['linear'],
						['coalesce', ['get', 'age_minutes'], 0],
						0, 0.7,
						120, 0.4,
						240, 0.15,
						360, 0
					],
					'circle-stroke-width': 0
				}
			});

			// --- Conflict heatmap layer — uses UNCLUSTERED source for accurate heat distribution ---
			// thermal_anomaly excluded: FIRMS fire detections have their own thermal-dots layer
			// and would otherwise create misleading large blobs at medium zoom.
			map.addLayer({
				id: 'conflict-heatmap',
				type: 'heatmap',
				source: 'events-unclustered',
				maxzoom: 7,
				filter: [
					'in', ['get', 'event_type'],
					['literal', ['conflict_event', 'nuclear_event', 'gps_interference']]
				],
				paint: {
					'heatmap-weight': [
						'match', ['get', 'severity'],
						'critical', 1.0,
						'high', 0.75,
						'medium', 0.5,
						'low', 0.25,
						0.3
					],
					'heatmap-intensity': [
						'interpolate', ['linear'], ['zoom'],
						0, 0.8,
						7, 2
					],
					'heatmap-radius': [
						'interpolate', ['linear'], ['zoom'],
						0, 10,
						4, 20,
						7, 30
					],
					'heatmap-color': [
						'interpolate', ['linear'], ['heatmap-density'],
						0, 'rgba(0,0,0,0)',
						0.2, 'rgba(254,178,76,0.3)',
						0.4, 'rgba(253,141,60,0.5)',
						0.6, 'rgba(252,78,42,0.6)',
						0.8, 'rgba(227,26,28,0.7)',
						1, 'rgba(189,0,38,0.8)'
					],
					'heatmap-opacity': [
						'interpolate', ['linear'], ['zoom'],
						4, 0.7,
						7, 0
					]
				}
			}, 'event-clusters'); // Insert below cluster layer

			// --- Position layers ---
			map.addSource('positions', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// Position trail lines
			map.addSource('trails', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			map.addLayer({
				id: 'position-trails',
				type: 'line',
				source: 'trails',
				paint: {
					'line-color': [
						'match',
						['get', 'type'],
						'flight-mil', '#f472b6',
						'flight', '#64748b',
						'#06b6d4'
					],
					'line-width': 1.5,
					'line-opacity': 0.4
				}
			});

			// Position arrows (symbol layer with rotation)
			// Civilian flights hidden at low zoom to declutter; military always visible
			map.addLayer({
				id: 'position-arrows',
				type: 'symbol',
				source: 'positions',
				layout: {
					'icon-image': ['get', 'icon_image'],
					'icon-size': [
						'case',
						['==', ['slice', ['coalesce', ['get', 'pos_type'], ''], 0, 4], 'mil-'], 0.9,
						['==', ['coalesce', ['get', 'pos_type'], ''], 'flight-mil'], 0.9,
						['==', ['coalesce', ['get', 'pos_type'], ''], 'flight'], 0.75,
						0.7
					],
					'icon-rotate': ['coalesce', ['get', 'heading'], 0],
					'icon-rotation-alignment': 'map',
					'icon-allow-overlap': true,
					'icon-ignore-placement': true
				},
				paint: {
					'icon-opacity': [
						'interpolate', ['linear'], ['zoom'],
						3, ['case', ['get', 'on_ground'], 0.15, ['get', 'is_military'], 1.0, 0.3],
						5, ['case', ['get', 'on_ground'], 0.15, ['get', 'is_military'], 1.0, 0.5],
						7, ['case', ['get', 'on_ground'], 0.15, ['get', 'is_military'], 1.0, 0.8]
					]
				}
			});

			// Position labels (show at higher zoom)
			map.addLayer({
				id: 'position-labels',
				type: 'symbol',
				source: 'positions',
				minzoom: 6,
				layout: {
					'text-field': ['get', 'label'],
					'text-size': 10,
					'text-offset': [0, 1.5],
					'text-anchor': 'top',
					'text-allow-overlap': false
				},
				paint: {
					'text-color': '#d1d5db',
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1
				}
			});

			// --- Military bases reference layer ---
			map.addSource('military-bases', {
				type: 'geojson',
				data: '/data/military-bases.geojson'
			});

			map.addLayer({
				id: 'bases-symbols',
				type: 'circle',
				source: 'military-bases',
				minzoom: 4,
				paint: {
					'circle-radius': [
						'interpolate', ['linear'], ['zoom'],
						4, 3,
						8, 5,
						12, 7
					],
					'circle-color': [
						'match', ['get', 'type'],
						'airfield', '#60a5fa',
						'naval_base', '#34d399',
						'base', '#fbbf24',
						'#94a3b8'
					],
					'circle-opacity': [
						'interpolate', ['linear'], ['zoom'],
						4, 0.3,
						6, 0.5,
						10, 0.7
					],
					'circle-stroke-width': 1,
					'circle-stroke-color': 'rgba(255,255,255,0.3)'
				},
				layout: {
					'visibility': 'none'
				}
			});

			map.addLayer({
				id: 'bases-labels',
				type: 'symbol',
				source: 'military-bases',
				minzoom: 6,
				layout: {
					'text-field': ['get', 'name'],
					'text-size': 9,
					'text-offset': [0, 1.2],
					'text-anchor': 'top',
					'text-allow-overlap': false,
					'visibility': 'none'
				},
				paint: {
					'text-color': '#94a3b8',
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1
				}
			});

			// Bases popup on click
			map.on('click', 'bases-symbols', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const coords = f.geometry.coordinates.slice();
				const p = f.properties;
				const html = `<div style="font-family:monospace;font-size:12px;color:#e5e7eb;max-width:220px;">
					<div style="font-weight:bold;color:#fbbf24;margin-bottom:4px;">${p.name || 'Unknown'}</div>
					<div style="font-size:11px;color:#9ca3af;">${p.type} ${p.country ? '(' + p.country + ')' : ''}</div>
					${p.operator ? `<div style="font-size:11px;color:#6b7280;margin-top:2px;">${p.operator}</div>` : ''}
				</div>`;
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '240px' })
					.setLngLat(coords)
					.setHTML(html)
					.addTo(map);
			});

			map.on('mouseenter', 'bases-symbols', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'bases-symbols', () => { map.getCanvas().style.cursor = ''; });

			// --- AIS monitoring zone overlays ---
			const aisZones = [
				{ name: 'Europe', south: 25, west: -15, north: 70, east: 45 },
				{ name: 'Middle East', south: 0, west: 30, north: 42, east: 75 },
				{ name: 'Indian Ocean', south: -15, west: 40, north: 15, east: 100 },
				{ name: 'East Asia', south: -10, west: 95, north: 50, east: 150 },
			];
			map.addSource('ais-zones', {
				type: 'geojson',
				data: {
					type: 'FeatureCollection',
					features: aisZones.map(z => ({
						type: 'Feature' as const,
						geometry: {
							type: 'Polygon' as const,
							coordinates: [[
								[z.west, z.south],
								[z.east, z.south],
								[z.east, z.north],
								[z.west, z.north],
								[z.west, z.south],
							]]
						},
						properties: {
							name: z.name,
							center_lng: (z.west + z.east) / 2,
							center_lat: (z.south + z.north) / 2,
						}
					}))
				}
			});
			map.addLayer({
				id: 'ais-zones-fill',
				type: 'fill',
				source: 'ais-zones',
				paint: {
					'fill-color': '#06b6d4',
					'fill-opacity': 0.06
				},
				layout: { 'visibility': 'none' }
			});
			map.addLayer({
				id: 'ais-zones-outline',
				type: 'line',
				source: 'ais-zones',
				paint: {
					'line-color': '#06b6d4',
					'line-width': 1.5,
					'line-opacity': 0.5,
					'line-dasharray': [4, 3]
				},
				layout: { 'visibility': 'none' }
			});
			// Label source: point features at zone centroids
			map.addSource('ais-zones-labels', {
				type: 'geojson',
				data: {
					type: 'FeatureCollection',
					features: aisZones.map(z => ({
						type: 'Feature' as const,
						geometry: {
							type: 'Point' as const,
							coordinates: [(z.west + z.east) / 2, (z.south + z.north) / 2]
						},
						properties: { name: z.name }
					}))
				}
			});
			map.addLayer({
				id: 'ais-zones-labels',
				type: 'symbol',
				source: 'ais-zones-labels',
				layout: {
					'text-field': ['get', 'name'],
					'text-size': 11,
					'text-anchor': 'center',
					'text-allow-overlap': true,
					'visibility': 'none'
				},
				paint: {
					'text-color': '#06b6d4',
					'text-opacity': 0.7,
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1
				}
			});

			// --- Click handlers ---

			// Click-to-zoom on cluster circles (Optimization #3)
			map.on('click', 'event-clusters', (e: any) => {
				const features = map.queryRenderedFeatures(e.point, { layers: ['event-clusters'] });
				if (!features.length) return;
				const clusterId = features[0].properties.cluster_id;
				(map.getSource('events') as any).getClusterExpansionZoom(clusterId, (err: any, zoom: number) => {
					if (err) return;
					map.easeTo({
						center: features[0].geometry.coordinates,
						zoom: zoom
					});
				});
			});

			map.on('mouseenter', 'event-clusters', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'event-clusters', () => { map.getCanvas().style.cursor = ''; });

			// Click handler for event popups
			map.on('click', 'events-circle', (e: any) => {
				if (!e.features?.length) return;
				const coords = e.features[0].geometry.coordinates.slice();
				const props = {
					...e.features[0].properties,
					longitude: coords[0],
					latitude: coords[1]
				};

				new maplibre.Popup({ className: 'sr-popup', maxWidth: '320px' })
					.setLngLat(coords)
					.setHTML(buildPopupHtml(props))
					.addTo(map);
			});

			// Click handler for impact site markers
			map.on('click', 'impact-sites', (e: any) => {
				if (!e.features?.length) return;
				const coords = e.features[0].geometry.coordinates.slice();
				const props = {
					...e.features[0].properties,
					longitude: coords[0],
					latitude: coords[1]
				};
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '320px' })
					.setLngLat(coords)
					.setHTML(buildPopupHtml(props))
					.addTo(map);
			});
			map.on('mouseenter', 'impact-sites', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'impact-sites', () => { map.getCanvas().style.cursor = ''; });

			// Click handler for NOTAM zone markers
			map.on('click', 'notam-zones', (e: any) => {
				if (!e.features?.length) return;
				const coords = e.features[0].geometry.coordinates.slice();
				const props = {
					...e.features[0].properties,
					longitude: coords[0],
					latitude: coords[1]
				};
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '320px' })
					.setLngLat(coords)
					.setHTML(buildPopupHtml(props))
					.addTo(map);
			});
			map.on('mouseenter', 'notam-zones', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'notam-zones', () => { map.getCanvas().style.cursor = ''; });

			// Click handler for NOTAM area polygons — use stored center for popup position
			map.on('click', 'notam-area-fill', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const centerLng = f.properties.center_lng;
				const centerLat = f.properties.center_lat;
				const popupCoords: [number, number] = (centerLng != null && centerLat != null)
					? [centerLng, centerLat]
					: [e.lngLat.lng, e.lngLat.lat];
				const props = {
					...f.properties,
					longitude: popupCoords[0],
					latitude: popupCoords[1]
				};
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '320px' })
					.setLngLat(popupCoords)
					.setHTML(buildPopupHtml(props))
					.addTo(map);
			});
			map.on('mouseenter', 'notam-area-fill', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'notam-area-fill', () => { map.getCanvas().style.cursor = ''; });

			// Click handler for position arrows — opens detail pane in right sidebar
			map.on('click', 'position-arrows', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const entityId = f.properties?.entity_id;

				// Look up full position entry from the store
				if (entityId) {
					const posEntry = mapStore.positions.get(entityId);
					if (posEntry) {
						uiStore.openPositionDetail(posEntry);
						return;
					}
				}

				// Fallback: show popup if position not found in store
				const coords = f.geometry.coordinates.slice();
				const p = f.properties;

				const hdg = p.heading != null ? `${Math.round(p.heading)}°` : 'N/A';
				const spd = p.speed != null ? `${Math.round(p.speed)}` : 'N/A';
				const alt = p.altitude != null ? `${Math.round(p.altitude).toLocaleString()} ft` : 'N/A';
				const lastSeen = p.last_seen ? escapeHtml(formatTimestamp(p.last_seen)) : '';

				const popupColor = p.affiliation && p.affiliation !== 'null'
					? (AFFILIATION_COLORS[p.affiliation] ?? DEFAULT_MIL_COLOR)
					: p.pos_type === 'flight-mil' ? DEFAULT_MIL_COLOR
					: p.pos_type === 'flight' ? CIVILIAN_COLOR
					: VESSEL_COLOR;

				const html = `<div style="font-family:monospace;font-size:12px;color:#e5e7eb;max-width:260px;">
					<div style="font-weight:bold;color:${popupColor};margin-bottom:4px;">${escapeHtml(p.label || p.entity_id)}</div>
					<div style="display:flex;flex-direction:column;gap:2px;">
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:60px;">Heading</span><span style="color:#d1d5db;">${hdg}</span></div>
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:60px;">Speed</span><span style="color:#d1d5db;">${spd}</span></div>
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:60px;">Altitude</span><span style="color:#d1d5db;">${alt}</span></div>
					</div>
					<div style="margin-top:4px;color:#6b7280;font-size:10px;">${p.source_type}${lastSeen ? ` · ${lastSeen}` : ''}</div>
				</div>`;

				new maplibre.Popup({ className: 'sr-popup', maxWidth: '280px' })
					.setLngLat(coords)
					.setHTML(html)
					.addTo(map);
			});

			map.on('mouseenter', 'events-circle', () => {
				map.getCanvas().style.cursor = 'pointer';
			});
			map.on('mouseleave', 'events-circle', () => {
				map.getCanvas().style.cursor = '';
			});
			map.on('mouseenter', 'position-arrows', () => {
				map.getCanvas().style.cursor = 'pointer';
			});
			map.on('mouseleave', 'position-arrows', () => {
				map.getCanvas().style.cursor = '';
			});

			// Thermal dot clicks
			map.on('click', 'thermal-dots', (e: any) => {
				if (!e.features?.length) return;
				const props = e.features[0].properties;
				const html = buildPopupHtml(props);
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '280px' })
					.setLngLat(e.lngLat)
					.setHTML(html)
					.addTo(map);
			});
			map.on('mouseenter', 'thermal-dots', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'thermal-dots', () => { map.getCanvas().style.cursor = ''; });

			// Fallback click handler — heatmap layers are non-interactive in MapLibre,
			// so find the closest event from the unclustered source when no interactive
			// layer handles the click.
			const interactiveLayers = [
				'event-clusters', 'events-circle', 'impact-sites', 'notam-zones',
				'notam-area-fill', 'thermal-dots', 'position-arrows', 'bases-symbols',
				'restricted-airspace-fill'
			];
			map.on('click', (e: any) => {
				// Skip if a specific layer already handled this click
				const hitLayers = map.queryRenderedFeatures(e.point)
					.filter((f: any) => interactiveLayers.includes(f.layer?.id));
				if (hitLayers.length > 0) return;

				// Find closest feature from the unclustered source within 30px
				const source = map.getSource('events-unclustered') as any;
				if (!source?._data?.features) return;
				const clickLng = e.lngLat.lng;
				const clickLat = e.lngLat.lat;
				// Convert 30px radius to approximate degree tolerance at current zoom
				const metersPerPx = 40075016.686 * Math.cos(clickLat * Math.PI / 180) / (512 * Math.pow(2, map.getZoom()));
				const tolerance = metersPerPx * 30 / 111320; // degrees
				const tolSq = tolerance * tolerance;

				let closest: any = null;
				let closestDist = Infinity;
				for (const f of source._data.features) {
					if (f.geometry?.type !== 'Point') continue;
					const [lng, lat] = f.geometry.coordinates;
					const d = (lng - clickLng) ** 2 + (lat - clickLat) ** 2;
					if (d < tolSq && d < closestDist) {
						closestDist = d;
						closest = f;
					}
				}
				if (!closest) return;

				const props = closest.properties;
				const html = buildPopupHtml(props);
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '280px' })
					.setLngLat(e.lngLat)
					.setHTML(html)
					.addTo(map);
			});

			// Reorder layers: NOTAM areas below clusters (like restricted airspace), events on top
			map.moveLayer('notam-area-fill', 'event-clusters');
			map.moveLayer('notam-area-outline', 'event-clusters');
			map.moveLayer('thermal-dots');
			map.moveLayer('incidents-glow');
			map.moveLayer('notam-zones');
			map.moveLayer('notam-labels');
			map.moveLayer('events-circle');
			map.moveLayer('pulse-ring');
			map.moveLayer('impact-sites');

			// Track viewport bounds for position filtering
			updateViewportBounds();
			map.on('moveend', () => {
				updateViewportBounds();
				// Optimization #6: Re-fetch geo events on significant viewport changes (throttled)
				refetchGeoForViewport();
			});

			mapLoaded = true;

			// Optimization #2: Pass map instance to the position interpolator for direct updates
			setMapInstance(map);

			// Optimization #1: Start batched event source updater (replaces the $effect)
			eventUpdateTimer = setInterval(updateEventSource, 2000);
			// Run once immediately so initial data appears without waiting
			updateEventSource();

			// Start periodic pulse cleanup — prunes expired entries and triggers re-render
			pulseInterval = setInterval(() => {
				mapStore.pruneRecentlyUpdated();
			}, 2000);
		});
	});

	// NOTE: The old $effect for event data is REMOVED — replaced by updateEventSource() interval above.
	// This eliminates the JSON.parse(JSON.stringify()) deep clone and clock-tick-driven churn.

	// Apply legend filter to event layers
	// geo_event and notam_event are excluded from events-circle — they have dedicated layers
	$effect(() => {
		if (!mapLoaded || !map) return;
		const hidden = mapStore.hiddenEventTypes;

		// Base filter: exclude special types from events-circle (they have dedicated layers)
		const excludeSpecial: any[] = ['!', ['in', ['get', 'event_type'], ['literal', ['geo_event', 'notam_event', 'thermal_anomaly']]]];
		// Cluster filter: only unclustered points
		const unclusteredFilter: any[] = ['!', ['has', 'point_count']];

		// Toggle NOTAM dot/label visibility based on event type legend
		const notamHidden = hidden.has('notam_event');
		const notamDotVis = notamHidden ? 'none' : 'visible';
		if (map.getLayer('notam-zones')) map.setLayoutProperty('notam-zones', 'visibility', notamDotVis);
		if (map.getLayer('notam-labels')) map.setLayoutProperty('notam-labels', 'visibility', notamDotVis);

		if (hidden.size === 0) {
			// No filter — show all except geo_event/notam_event (handled by dedicated layers)
			map.setFilter('events-circle', ['all', unclusteredFilter, excludeSpecial]);
			map.setFilter('incidents-glow', ['all', unclusteredFilter,
				['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:']
			]);
		} else {
			const hiddenArr = [...hidden];
			const baseFilter: any[] = ['!', ['in', ['get', 'event_type'], ['literal', hiddenArr]]];
			map.setFilter('events-circle', ['all', unclusteredFilter, baseFilter, excludeSpecial]);
			map.setFilter('incidents-glow', [
				'all',
				unclusteredFilter,
				['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
				baseFilter
			]);
		}
	});

	// Toggle heatmap visibility
	$effect(() => {
		if (!mapLoaded || !map?.getLayer('conflict-heatmap')) return;
		map.setLayoutProperty(
			'conflict-heatmap',
			'visibility',
			mapStore.heatmapVisible ? 'visible' : 'none'
		);
	});

	// Toggle military bases reference layer
	$effect(() => {
		if (!mapLoaded) return;
		const vis = mapStore.basesVisible ? 'visible' : 'none';
		if (map?.getLayer('bases-symbols')) map.setLayoutProperty('bases-symbols', 'visibility', vis);
		if (map?.getLayer('bases-labels')) map.setLayoutProperty('bases-labels', 'visibility', vis);
	});

	// Toggle impact sites visibility
	$effect(() => {
		if (!mapLoaded) return;
		const vis = mapStore.impactSitesVisible ? 'visible' : 'none';
		if (map?.getLayer('impact-sites')) map.setLayoutProperty('impact-sites', 'visibility', vis);
	});

	// Toggle AIS monitoring zones
	$effect(() => {
		if (!mapLoaded) return;
		const vis = mapStore.aisZonesVisible ? 'visible' : 'none';
		for (const id of ['ais-zones-fill', 'ais-zones-outline', 'ais-zones-labels']) {
			if (map?.getLayer(id)) map.setLayoutProperty(id, 'visibility', vis);
		}
	});

	// --- FIR Boundaries overlay (lazy loaded) ---
	async function loadFirBoundaries() {
		if (mapStore.firBoundariesLoaded || !map) return;
		try {
			const resp = await fetch('/data/fir-boundaries.json');
			const data = await resp.json();
			map.addSource('fir-boundaries', { type: 'geojson', data });

			// Dashed boundary lines
			map.addLayer({
				id: 'fir-boundaries-line',
				type: 'line',
				source: 'fir-boundaries',
				paint: {
					'line-color': [
						'match', ['get', 'type'],
						'FIR', '#6b7280',
						'UIR', '#60a5fa',
						'#6b7280'
					],
					'line-width': 1,
					'line-opacity': 0.5,
					'line-dasharray': [4, 3]
				},
				layout: { 'visibility': 'visible' }
			}, 'event-clusters'); // Insert below event layers

			// Compute centroids for label placement
			const labelFeatures: any[] = [];
			for (const f of data.features) {
				if (!f.geometry?.coordinates?.[0]) continue;
				const ring = f.geometry.coordinates[0];
				let sumLng = 0, sumLat = 0, n = 0;
				for (const coord of ring) {
					sumLng += coord[0];
					sumLat += coord[1];
					n++;
				}
				if (n > 0) {
					labelFeatures.push({
						type: 'Feature',
						geometry: { type: 'Point', coordinates: [sumLng / n, sumLat / n] },
						properties: { name: f.properties?.name ?? f.properties?.icao ?? '' }
					});
				}
			}
			map.addSource('fir-labels', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: labelFeatures }
			});
			map.addLayer({
				id: 'fir-labels',
				type: 'symbol',
				source: 'fir-labels',
				minzoom: 4,
				layout: {
					'text-field': ['get', 'name'],
					'text-size': 9,
					'text-anchor': 'center',
					'text-allow-overlap': false,
					'visibility': 'visible'
				},
				paint: {
					'text-color': '#6b7280',
					'text-opacity': 0.6,
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1
				}
			});

			mapStore.firBoundariesLoaded = true;
		} catch (e) {
			console.warn('Failed to load FIR boundaries:', e);
		}
	}

	// Toggle FIR boundaries
	$effect(() => {
		if (!mapLoaded) return;
		if (mapStore.firBoundariesVisible && !mapStore.firBoundariesLoaded) {
			loadFirBoundaries();
			return;
		}
		const vis = mapStore.firBoundariesVisible ? 'visible' : 'none';
		for (const id of ['fir-boundaries-line', 'fir-labels']) {
			if (map?.getLayer(id)) map.setLayoutProperty(id, 'visibility', vis);
		}
	});

	// --- Restricted Airspace overlay (lazy loaded — 9.7MB) ---
	async function loadRestrictedAirspace() {
		if (mapStore.restrictedAirspaceLoaded || !map) return;
		try {
			const resp = await fetch('/data/restricted-airspace.json');
			const data = await resp.json();
			map.addSource('restricted-airspace', { type: 'geojson', data });

			// Color-coded fill by zone type
			map.addLayer({
				id: 'restricted-airspace-fill',
				type: 'fill',
				source: 'restricted-airspace',
				paint: {
					'fill-color': [
						'match', ['get', 'type'],
						'PROHIBITED', 'rgba(239, 68, 68, 0.12)',
						'RESTRICTED', 'rgba(249, 115, 22, 0.08)',
						'DANGER', 'rgba(234, 179, 8, 0.06)',
						'WARNING', 'rgba(234, 179, 8, 0.04)',
						'rgba(156, 163, 175, 0.04)'
					],
					'fill-opacity': [
						'interpolate', ['linear'], ['zoom'],
						3, 0.3,
						6, 0.6,
						10, 0.8
					]
				},
				layout: { 'visibility': 'visible' }
			}, 'event-clusters');

			// Border lines
			map.addLayer({
				id: 'restricted-airspace-line',
				type: 'line',
				source: 'restricted-airspace',
				paint: {
					'line-color': [
						'match', ['get', 'type'],
						'PROHIBITED', '#ef4444',
						'RESTRICTED', '#f97316',
						'DANGER', '#eab308',
						'WARNING', '#eab308',
						'#9ca3af'
					],
					'line-width': [
						'interpolate', ['linear'], ['zoom'],
						3, 0.5,
						8, 1,
						12, 1.5
					],
					'line-opacity': 0.5
				},
				layout: { 'visibility': 'visible' }
			}, 'event-clusters');

			// Click handler for restricted zones
			const maplibre = await import('maplibre-gl');
			map.on('click', 'restricted-airspace-fill', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const p = f.properties;
				const typeColors: Record<string, string> = {
					PROHIBITED: '#ef4444', RESTRICTED: '#f97316',
					DANGER: '#eab308', WARNING: '#eab308'
				};
				const typeColor = typeColors[p.type] ?? '#9ca3af';
				const html = `<div style="font-family:monospace;font-size:12px;color:#e5e7eb;max-width:260px;">
					<div style="font-weight:bold;color:${typeColor};margin-bottom:4px;">${p.type ?? 'AIRSPACE'}</div>
					<div style="font-size:11px;color:#d1d5db;margin-bottom:2px;">${p.name ?? ''}</div>
					${p.designator ? `<div style="font-size:11px;color:#9ca3af;">${p.designator}</div>` : ''}
					${p.upper_limit || p.lower_limit ? `<div style="font-size:10px;color:#6b7280;margin-top:4px;">${p.lower_limit ?? 'GND'} — ${p.upper_limit ?? 'UNL'}</div>` : ''}
					${p.country ? `<div style="font-size:10px;color:#6b7280;">${p.country}</div>` : ''}
				</div>`;
				new maplibre.Popup({ className: 'sr-popup', maxWidth: '280px' })
					.setLngLat(e.lngLat)
					.setHTML(html)
					.addTo(map);
			});
			map.on('mouseenter', 'restricted-airspace-fill', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'restricted-airspace-fill', () => { map.getCanvas().style.cursor = ''; });

			mapStore.restrictedAirspaceLoaded = true;
		} catch (e) {
			console.warn('Failed to load restricted airspace:', e);
		}
	}

	// Toggle restricted airspace
	$effect(() => {
		if (!mapLoaded) return;
		if (mapStore.restrictedAirspaceVisible && !mapStore.restrictedAirspaceLoaded) {
			loadRestrictedAirspace();
			return;
		}
		const vis = mapStore.restrictedAirspaceVisible ? 'visible' : 'none';
		for (const id of ['restricted-airspace-fill', 'restricted-airspace-line']) {
			if (map?.getLayer(id)) map.setLayoutProperty(id, 'visibility', vis);
		}
	});

	// Toggle NOTAM area overlays
	$effect(() => {
		if (!mapLoaded) return;
		const vis = mapStore.notamAreasVisible ? 'visible' : 'none';
		for (const id of ['notam-area-fill', 'notam-area-outline']) {
			if (map?.getLayer(id)) map.setLayoutProperty(id, 'visibility', vis);
		}
	});

	// NOTE: The old position $effect is REMOVED — the position interpolator now updates
	// MapLibre directly via setMapInstance() (Optimization #2). The store is only updated
	// on actual poll responses (every 30s), not on interpolation ticks.

	// Update trail lines when history changes
	// Uses entityMeta (updated on poll ~30s) instead of positions (updated at 10Hz by interpolator)
	$effect(() => {
		if (!mapLoaded || !map?.getSource('trails')) return;
		const meta = mapStore.entityMeta;
		const features: any[] = [];
		for (const [entityId, trail] of mapStore.positionHistory) {
			if (trail.length < 2) continue;
			const em = meta.get(entityId);
			const trailFlightSources = ['airplaneslive', 'adsb-lol', 'adsb-fi', 'opensky'];
			const isMil = em?.military === true;
			const isFlight =
				(em?.source_type != null && trailFlightSources.includes(em.source_type)) ||
				em?.source_type?.includes('flight');
			const posType = isMil ? 'flight-mil' : isFlight ? 'flight' : 'vessel';

			features.push({
				type: 'Feature',
				geometry: {
					type: 'LineString',
					coordinates: trail.map((p) => [p.lng, p.lat])
				},
				properties: {
					entity_id: entityId,
					type: posType
				}
			});
		}
		map.getSource('trails').setData({
			type: 'FeatureCollection',
			features
		});
	});

	// Fly to location when requested
	$effect(() => {
		const target = mapStore.flyToTarget;
		if (target && map) {
			map.flyTo({
				center: target.center,
				zoom: target.zoom,
				duration: 1500
			});
			mapStore.flyToTarget = null;
		}
	});

	onDestroy(() => {
		if (pulseInterval) clearInterval(pulseInterval);
		if (eventUpdateTimer) clearInterval(eventUpdateTimer);
		delete (window as any).__srOpenDetail;
		delete (window as any).__srDetailProps;
		map?.remove();
	});
</script>

<div class="relative h-full w-full">
	<div class="h-full w-full" bind:this={container}></div>
	<div class="absolute bottom-2 left-1/2 z-10 w-[60%] -translate-x-1/2">
		<TimelineSlider />
	</div>
</div>
