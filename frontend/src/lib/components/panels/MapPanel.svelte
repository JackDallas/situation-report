<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { eventStore } from '$lib/stores/events.svelte';
	import { situationsStore } from '$lib/stores/situations.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import TimelineSlider from '$lib/components/shared/TimelineSlider.svelte';
	import { getOutlink, getEventDetails, escapeHtml } from '$lib/services/outlinks';
	import { typeColorMap, formatTimestamp, formatAbsoluteTime } from '$lib/services/event-display';
	import { uiStore } from '$lib/stores/ui.svelte';
	import { setMapInstance } from '$lib/services/position-interpolator';
	import { refetchGeoForViewport } from '$lib/services/ws';
	import type { SituationEvent } from '$lib/types/events';
	import { AFFILIATION_COLORS, DEFAULT_MIL_COLOR, CIVILIAN_COLOR, VESSEL_COLOR } from '$lib/config/colors';
	import { satelliteStore } from '$lib/services/satellites.svelte';
	import type { Map as MapLibreMap, GeoJSONSource } from 'maplibre-gl';

	let container: HTMLDivElement;
	let map: MapLibreMap;
	let mapLoaded = $state(false);

	/**
	 * Generate a GeoJSON Polygon approximating a circle on the Earth's surface.
	 */
	function circlePolygon(
		centerLng: number,
		centerLat: number,
		radiusNm: number,
		numPoints = 64
	): [number, number][] {
		const R = 6371;
		const d = (radiusNm * 1.852) / R;
		const lat = (centerLat * Math.PI) / 180;
		const lng = (centerLng * Math.PI) / 180;
		const coords: [number, number][] = [];
		for (let i = 0; i <= numPoints; i++) {
			const bearing = (2 * Math.PI * i) / numPoints;
			const pLat = Math.asin(
				Math.sin(lat) * Math.cos(d) + Math.cos(lat) * Math.sin(d) * Math.cos(bearing)
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

		const raw = mapStore.geoData;
		const cursorMs = mapStore.timeCursor.getTime();

		const features: import('$lib/types/events').GeoJSONFeature[] = [];
		const notamAreaFeatures: GeoJSON.Feature<GeoJSON.Polygon>[] = [];

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

			// Create a lightweight wrapper sharing the original geometry reference
			const enrichedProps = {
				...f.properties,
				age_minutes: ageMinutes
			};

			const enrichedFeature = {
				type: 'Feature',
				geometry: f.geometry,
				properties: enrichedProps
			};
			features.push(enrichedFeature);

			// Build NOTAM area polygons from radius data
			const notamRadius = f.properties?.notam_radius_nm;
			if (f.properties?.event_type === 'notam_event'
				&& typeof notamRadius === 'number' && notamRadius > 0
				&& f.geometry?.type === 'Point'
				&& f.geometry.coordinates
			) {
				const [lng, lat] = f.geometry.coordinates;
				notamAreaFeatures.push({
					type: 'Feature',
					geometry: { type: 'Polygon', coordinates: [circlePolygon(lng, lat, notamRadius)] },
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
						center_lng: lng,
						center_lat: lat,
						radius_nm: notamRadius
					}
				});
			}

		}

		const allFeatures = { type: 'FeatureCollection' as const, features };

		// Update both sources with all features (no clustering)
		// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
		(map.getSource('events') as GeoJSONSource | undefined)?.setData(allFeatures);
		(map.getSource('events-unclustered') as GeoJSONSource | undefined)?.setData(allFeatures);

		// Update NOTAM area polygons
		// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
		(map.getSource('notam-areas') as GeoJSONSource | undefined)?.setData({
			type: 'FeatureCollection' as const, features: notamAreaFeatures
		});
	}

	/** Build popup HTML from feature properties. */
	function buildPopupHtml(props: Record<string, unknown>): string {
		// Look up payload from the separate cache (stripped from GeoJSON for performance)
		let payload: Record<string, unknown> = {};
		const sourceId = props.source_id as string | null;
		if (sourceId) {
			payload = mapStore.payloadCache.get(sourceId) ?? {};
		}
		// Fallback: if payload was still on the feature properties (e.g. from older data)
		if (Object.keys(payload).length === 0 && props.payload) {
			try {
				payload =
					typeof props.payload === 'string'
						? JSON.parse(props.payload)
						: (props.payload as Record<string, unknown> ?? {});
			} catch {
				payload = {};
			}
		}

		const pseudoEvent: SituationEvent = {
			event_time: (props.event_time as string) ?? '',
			source_type: (props.source_type as string) ?? '',
			source_id: (props.source_id as string | null) ?? null,
			latitude: null,
			longitude: null,
			region_code: (props.region_code as string | null) ?? null,
			entity_id: (props.entity_id as string | null) ?? null,
			entity_name: (props.entity_name as string | null) ?? null,
			event_type: (props.event_type as string) ?? '',
			severity: (props.severity as string) ?? 'low',
			confidence: (props.confidence as number | null) ?? null,
			tags: [],
			title: (props.title as string | null) ?? null,
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

		const detailIdx = (window.__srDetailProps ??= []).length;
		window.__srDetailProps.push(props);
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
		mapInstance: MapLibreMap,
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
		window.__srDetailProps = [];
		window.__srOpenDetail = (idx: number) => {
			const props = window.__srDetailProps?.[idx];
			if (!props) return;

			// Check if this is an incident feature (event_type starts with "incident:")
			const isIncident = typeof props.event_type === 'string' && props.event_type.startsWith('incident:');
			if (isIncident && props.entity_id) {
				const foundIncident = eventStore.incidents.find((i) => i.id === props.entity_id);
				if (foundIncident) {
					eventStore.selectedEvent = null;
					eventStore.selectedIncident = foundIncident;
					uiStore.openPanel('incident-detail');
					return;
				}
			}

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

		// Register global bridge for situation popup -> drawer navigation
		window.__srOpenSituation = (situationId: string) => {
			const sit = situationsStore.situationById.get(situationId);
			if (sit) {
				situationsStore.selectedSituation = sit;
			}
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
			map.addSource('events', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// Unclustered events source — used by heatmap (needs raw points, not cluster centroids)
			map.addSource('events-unclustered', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// --- NOTAM area polygons — geographic circles styled like restricted airspace ---
			map.addSource('notam-areas', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			map.addLayer({
				id: 'notam-area-fill',
				type: 'fill',
				source: 'notam-areas',
				paint: {
					'fill-color': [
						'match', ['get', 'severity'],
						'critical', 'rgba(217, 70, 239, 0.12)',
						'high', 'rgba(168, 85, 247, 0.08)',
						'medium', 'rgba(168, 85, 247, 0.06)',
						'rgba(139, 92, 246, 0.04)'
					],
					'fill-opacity': [
						'interpolate', ['linear'], ['zoom'],
						3, 0.3,
						6, 0.6,
						10, 0.8
					]
				}
			});

			map.addLayer({
				id: 'notam-area-line',
				type: 'line',
				source: 'notam-areas',
				paint: {
					'line-color': [
						'match', ['get', 'severity'],
						'critical', '#d946ef',
						'high', '#a855f7',
						'#8b5cf6'
					],
					'line-width': [
						'interpolate', ['linear'], ['zoom'],
						3, 0.5,
						8, 1,
						12, 1.5
					],
					'line-opacity': 0.5
				}
			});

			// Incident glow layer
			map.addLayer({
				id: 'incidents-glow',
				type: 'circle',
				source: 'events',
				filter: [
					'==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'
				],
				paint: {
					'circle-radius': 14,
					'circle-color': '#ef4444',
					'circle-opacity': 0.25,
					'circle-stroke-width': 0
				}
			});

			// Main event circles — all events rendered individually
			map.addLayer({
				id: 'events-circle',
				type: 'circle',
				source: 'events',
				filter: ['!=', ['get', 'event_type'], 'thermal_anomaly'],
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
						1
					],
					'circle-stroke-color': [
						'case',
						['==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'],
						'#ffffff',
						'rgba(255, 255, 255, 0.3)'
					]
				}
			});

			// --- Impact sites layer — GeoConfirmed confirmed events (subtle markers) ---
			map.addLayer({
				id: 'impact-sites',
				type: 'circle',
				source: 'events-unclustered',
				filter: ['==', ['get', 'event_type'], 'geo_event'],
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


			// --- Thermal anomaly layer — FRP-scaled dots for FIRMS fire detections ---
			// FRP (Fire Radiative Power, MW): median ~5, P95 ~26, max ~581. Log-scale normalization.
			// normalized = log10(frp + 1) / log10(600) ≈ 0..1
			map.addLayer({
				id: 'thermal-dots',
				type: 'circle',
				source: 'events-unclustered',
				filter: ['==', ['get', 'event_type'], 'thermal_anomaly'],
				paint: {
					'circle-radius': [
						'interpolate', ['linear'], ['zoom'],
						3, ['*', ['interpolate', ['linear'],
							['coalesce', ['get', 'frp'], 5],
							0, 1.0,  5, 1.0,  25, 1.3,  100, 1.8,  500, 3.5
						], 2],
						6, ['*', ['interpolate', ['linear'],
							['coalesce', ['get', 'frp'], 5],
							0, 1.0,  5, 1.0,  25, 1.3,  100, 1.8,  500, 3.5
						], 3],
						10, ['*', ['interpolate', ['linear'],
							['coalesce', ['get', 'frp'], 5],
							0, 1.0,  5, 1.0,  25, 1.3,  100, 1.8,  500, 3.5
						], 4],
						14, ['*', ['interpolate', ['linear'],
							['coalesce', ['get', 'frp'], 5],
							0, 1.0,  5, 1.0,  25, 1.3,  100, 1.8,  500, 3.5
						], 6]
					],
					'circle-color': [
						'interpolate', ['linear'],
						['coalesce', ['get', 'frp'], 5],
						0, '#fbbf24',    // yellow — low FRP
						5, '#fbbf24',
						25, '#f97316',   // orange — moderate
						100, '#ef4444',  // red — high
						500, '#dc2626'   // bright red — extreme
					],
					'circle-opacity': [
						'*',
						// Age decay
						['interpolate', ['linear'],
							['coalesce', ['get', 'age_minutes'], 0],
							0, 1.0,
							120, 0.6,
							240, 0.25,
							360, 0
						],
						// FRP intensity modulation
						['interpolate', ['linear'],
							['coalesce', ['get', 'frp'], 5],
							0, 0.5,
							25, 0.7,
							100, 0.85,
							500, 0.95
						]
					],
					'circle-stroke-width': 0
				}
			});

			// --- Intel heatmap layer — uses UNCLUSTERED source for accurate heat distribution ---
			// thermal_anomaly excluded: FIRMS fire detections have their own thermal-dots layer
			// and would otherwise create misleading large blobs at medium zoom.
			map.addLayer({
				id: 'conflict-heatmap',
				type: 'heatmap',
				source: 'events-unclustered',
				maxzoom: 7,
				filter: [
					'in', ['get', 'event_type'],
					['literal', ['conflict_event', 'nuclear_event', 'gps_interference',
						'seismic_event', 'telegram_message', 'bluesky_post',
						'maritime_security', 'geo_event', 'news_article',
						'internet_outage', 'censorship_event', 'threat_intel']]
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
			}, 'events-circle'); // Insert below event dots

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
						3, ['case', ['get', 'is_stale'], 0.1, ['get', 'on_ground'], 0.15, ['get', 'is_military'], 1.0, 0.3],
						5, ['case', ['get', 'is_stale'], 0.1, ['get', 'on_ground'], 0.15, ['get', 'is_military'], 1.0, 0.5],
						7, ['case', ['get', 'is_stale'], 0.15, ['get', 'on_ground'], 0.15, ['get', 'is_military'], 1.0, 0.8]
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

			// --- FIRMS satellite position layer ---
			map.addSource('satellite-positions', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// Satellite dot — white circle with cyan glow
			map.addLayer({
				id: 'satellite-dots',
				type: 'circle',
				source: 'satellite-positions',
				paint: {
					'circle-radius': 5,
					'circle-color': '#ffffff',
					'circle-opacity': 0.95,
					'circle-stroke-width': 2,
					'circle-stroke-color': '#22d3ee',
					'circle-stroke-opacity': 0.7
				}
			});

			// Satellite labels
			map.addLayer({
				id: 'satellite-labels',
				type: 'symbol',
				source: 'satellite-positions',
				layout: {
					'text-field': ['get', 'name'],
					'text-size': 9,
					'text-offset': [0, 1.4],
					'text-anchor': 'top',
					'text-allow-overlap': true
				},
				paint: {
					'text-color': '#22d3ee',
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1,
					'text-opacity': 0.85
				}
			});

			// Satellite orbit trail (past path — solid cyan line)
			map.addSource('satellite-trail', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});
			map.addLayer({
				id: 'satellite-trail-line',
				type: 'line',
				source: 'satellite-trail',
				paint: {
					'line-color': '#22d3ee',
					'line-width': 1.5,
					'line-opacity': 0.6
				}
			});

			// Satellite orbit future (predicted path — dashed cyan line)
			map.addSource('satellite-future', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});
			map.addLayer({
				id: 'satellite-future-line',
				type: 'line',
				source: 'satellite-future',
				paint: {
					'line-color': '#22d3ee',
					'line-width': 1.5,
					'line-opacity': 0.4,
					'line-dasharray': [4, 4]
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

			// --- Situation cluster markers ---
			map.addSource('situations', {
				type: 'geojson',
				data: { type: 'FeatureCollection', features: [] }
			});

			// Outer glow ring for situations
			map.addLayer({
				id: 'situation-glow',
				type: 'circle',
				source: 'situations',
				paint: {
					'circle-radius': [
						'interpolate', ['linear'], ['zoom'],
						2, ['match', ['get', 'severity'], 'critical', 16, 'high', 14, 12],
						6, ['match', ['get', 'severity'], 'critical', 22, 'high', 18, 15],
						10, ['match', ['get', 'severity'], 'critical', 28, 'high', 24, 20]
					],
					'circle-color': [
						'match', ['get', 'severity'],
						'critical', 'rgba(239, 68, 68, 0.15)',
						'high', 'rgba(249, 115, 22, 0.12)',
						'medium', 'rgba(234, 179, 8, 0.10)',
						'rgba(107, 114, 128, 0.08)'
					],
					'circle-stroke-width': 0
				}
			});

			// Inner dot for situation centroid
			map.addLayer({
				id: 'situation-dots',
				type: 'circle',
				source: 'situations',
				paint: {
					'circle-radius': [
						'interpolate', ['linear'], ['zoom'],
						2, 5,
						6, 7,
						10, 9
					],
					'circle-color': [
						'match', ['get', 'severity'],
						'critical', '#ef4444',
						'high', '#f97316',
						'medium', '#eab308',
						'#6b7280'
					],
					'circle-opacity': 0.9,
					'circle-stroke-width': 2,
					'circle-stroke-color': 'rgba(255, 255, 255, 0.6)'
				}
			});

			// Situation labels — show at medium zoom
			map.addLayer({
				id: 'situation-labels',
				type: 'symbol',
				source: 'situations',
				minzoom: 4,
				layout: {
					'text-field': ['get', 'title'],
					'text-size': [
						'interpolate', ['linear'], ['zoom'],
						4, 10,
						8, 12
					],
					'text-offset': [0, 1.6],
					'text-anchor': 'top',
					'text-allow-overlap': false,
					'text-max-width': 14
				},
				paint: {
					'text-color': [
						'match', ['get', 'severity'],
						'critical', '#fca5a5',
						'high', '#fdba74',
						'medium', '#fde047',
						'#d1d5db'
					],
					'text-halo-color': '#1a1a2e',
					'text-halo-width': 1.5,
					'text-opacity': [
						'interpolate', ['linear'], ['zoom'],
						4, 0.7,
						7, 0.9
					]
				}
			});

			// --- Click handlers ---

			// Click handler for situation markers
			map.on('click', 'situation-dots', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const p = f.properties;
				const coords = f.geometry.coordinates.slice();

				const sevColor = p.severity === 'critical' ? '#f87171'
					: p.severity === 'high' ? '#fb923c'
					: p.severity === 'medium' ? '#facc15'
					: '#9ca3af';
				const phaseLabel = p.phase ? p.phase.charAt(0).toUpperCase() + p.phase.slice(1) : '';

				const html = `<div style="font-family:monospace;font-size:12px;color:#e5e7eb;max-width:280px;">
					<div style="display:flex;align-items:center;gap:6px;margin-bottom:6px;">
						<span style="font-weight:bold;color:${sevColor};">${escapeHtml(p.title || 'Situation')}</span>
					</div>
					<div style="display:flex;flex-direction:column;gap:2px;">
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">Severity</span><span style="color:${sevColor};">${escapeHtml((p.severity || '').toUpperCase())}</span></div>
						${phaseLabel ? `<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">Phase</span><span style="color:#d1d5db;">${escapeHtml(phaseLabel)}</span></div>` : ''}
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">Events</span><span style="color:#d1d5db;">${p.event_count ?? 0}</span></div>
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">Sources</span><span style="color:#d1d5db;">${p.source_count ?? 0}</span></div>
					</div>
					<button onclick="window.__srOpenSituation('${escapeHtml(p.situation_id || '')}')" style="margin-top:6px;padding:3px 8px;background:rgba(59,130,246,0.15);color:#60a5fa;border:none;border-radius:4px;font-size:11px;cursor:pointer;font-family:monospace;">View Situation</button>
				</div>`;

				new maplibre.Popup({ className: 'sr-popup', maxWidth: '300px' })
					.setLngLat(coords)
					.setHTML(html)
					.addTo(map);
			});

			map.on('mouseenter', 'situation-dots', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'situation-dots', () => { map.getCanvas().style.cursor = ''; });

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

			// Click handler for NOTAM area polygons — use stored center for popup
			map.on('click', 'notam-area-fill', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const popupCoords: [number, number] = (f.properties.center_lng != null && f.properties.center_lat != null)
					? [f.properties.center_lng, f.properties.center_lat]
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

			// Click handler for position arrows — opens detail pane + loads trail
			map.on('click', 'position-arrows', (e: any) => {
				if (!e.features?.length) return;
				const f = e.features[0];
				const entityId = f.properties?.entity_id;

				// Look up full position entry from the store
				if (entityId) {
					const posEntry = mapStore.positions.get(entityId);
					if (posEntry) {
						uiStore.openPositionDetail(posEntry);
						// Auto-load 2h trail on click
						mapStore.loadEntityTrail(entityId, 2);
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

			// Satellite dot clicks — show details popup + orbit trail
			map.on('click', 'satellite-dots', (e: any) => {
				if (!e.features?.length) return;
				const p = e.features[0].properties;
				const noradId = p.norad_id;
				const name = p.name ?? 'Unknown';
				const alt = p.altitude_km ?? 0;

				// Toggle orbit path
				satelliteStore.selectSatellite(noradId);

				// Compute speed from current position data
				const sat = satelliteStore.positions.find((s) => s.norad_id === noradId);
				const latStr = sat ? sat.lat.toFixed(2) + '°' : 'N/A';
				const lonStr = sat ? sat.lon.toFixed(2) + '°' : 'N/A';

				const html = `<div style="font-family:monospace;font-size:12px;color:#e5e7eb;max-width:240px;">
					<div style="font-weight:bold;color:#22d3ee;margin-bottom:4px;">${escapeHtml(name)}</div>
					<div style="display:flex;flex-direction:column;gap:2px;">
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">NORAD ID</span><span style="color:#d1d5db;">${noradId}</span></div>
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">Altitude</span><span style="color:#d1d5db;">${Math.round(alt).toLocaleString()} km</span></div>
						<div style="display:flex;gap:8px;font-size:11px;"><span style="color:#6b7280;min-width:70px;">Position</span><span style="color:#d1d5db;">${latStr}, ${lonStr}</span></div>
					</div>
					<div style="margin-top:6px;color:#6b7280;font-size:10px;">FIRMS thermal imaging satellite</div>
					<div style="margin-top:2px;color:#22d3ee;font-size:10px;">Showing ±45 min orbit path</div>
				</div>`;

				new maplibre.Popup({ className: 'sr-popup', maxWidth: '260px' })
					.setLngLat(e.lngLat)
					.setHTML(html)
					.addTo(map);
			});
			map.on('mouseenter', 'satellite-dots', () => { map.getCanvas().style.cursor = 'pointer'; });
			map.on('mouseleave', 'satellite-dots', () => { map.getCanvas().style.cursor = ''; });

			// Fallback click handler — heatmap layers are non-interactive in MapLibre,
			// so find the closest event from the unclustered source when no interactive
			// layer handles the click.
			const interactiveLayers = [
				'events-circle', 'impact-sites', 'situation-dots',
				'notam-area-fill', 'thermal-dots', 'position-arrows', 'bases-symbols',
				'restricted-airspace-fill', 'satellite-dots'
			];
			map.on('click', (e: any) => {
				// Skip if a specific layer already handled this click
				const hitLayers = map.queryRenderedFeatures(e.point)
					.filter((f: any) => interactiveLayers.includes(f.layer?.id));
				if (hitLayers.length > 0) return;

				// Find closest feature from the unclustered source within 30px
				// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
				const source = map.getSource('events-unclustered') as GeoJSONSource | undefined;
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

			// Reorder layers: NOTAM areas below events, events on top
			map.moveLayer('notam-area-fill', 'events-circle');
			map.moveLayer('thermal-dots');
			map.moveLayer('incidents-glow');
			map.moveLayer('notam-area-line', 'events-circle');
			map.moveLayer('events-circle');
			map.moveLayer('impact-sites');
			// Situation markers on top of everything
			map.moveLayer('situation-glow');
			map.moveLayer('situation-dots');
			map.moveLayer('situation-labels');

			// Track viewport bounds for position filtering
			updateViewportBounds();
			map.on('moveend', () => {
				updateViewportBounds();
				// Optimization #6: Re-fetch geo events on significant viewport changes (throttled)
				refetchGeoForViewport();
			});

			mapLoaded = true;

			// Start satellite position tracking (fetches TLEs, propagates every 3s)
			satelliteStore.start();

			// Optimization #2: Pass map instance to the position interpolator for direct updates
			setMapInstance(map);

			// Optimization #1: Start batched event source updater (replaces the $effect)
			eventUpdateTimer = setInterval(updateEventSource, 2000);
			// Run once immediately so initial data appears without waiting
			updateEventSource();

		});
	});

	// NOTE: The old $effect for event data is REMOVED — replaced by updateEventSource() interval above.
	// This eliminates the JSON.parse(JSON.stringify()) deep clone and clock-tick-driven churn.

	// Apply legend filter to event layers
	$effect(() => {
		if (!mapLoaded || !map) return;
		const hidden = mapStore.hiddenEventTypes;

		// Exclude types handled by dedicated layers
		const excludeSpecial: any[] = ['!', ['in', ['get', 'event_type'], ['literal', ['geo_event', 'notam_event', 'thermal_anomaly']]]];

		// Toggle NOTAM area visibility based on event type legend
		const notamHidden = hidden.has('notam_event');
		const notamVis = notamHidden ? 'none' : 'visible';
		if (map.getLayer('notam-area-fill')) map.setLayoutProperty('notam-area-fill', 'visibility', notamVis);
		if (map.getLayer('notam-area-line')) map.setLayoutProperty('notam-area-line', 'visibility', notamVis);

		if (hidden.size === 0) {
			map.setFilter('events-circle', excludeSpecial);
			map.setFilter('incidents-glow', [
				'==', ['slice', ['coalesce', ['get', 'event_type'], ''], 0, 9], 'incident:'
			]);
		} else {
			const hiddenArr = [...hidden];
			const baseFilter: any[] = ['!', ['in', ['get', 'event_type'], ['literal', hiddenArr]]];
			map.setFilter('events-circle', ['all', baseFilter, excludeSpecial]);
			map.setFilter('incidents-glow', [
				'all',
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

	// Update situation markers when situations change
	$effect(() => {
		if (!mapLoaded || !map?.getSource('situations')) return;
		const situations = situationsStore.situations;
		const features = situations
			.filter((s) => s.latitude != null && s.longitude != null && s.latitude !== 0 && s.longitude !== 0 && !s.parentId)
			.map((s) => ({
				type: 'Feature' as const,
				geometry: {
					type: 'Point' as const,
					coordinates: [s.longitude!, s.latitude!]
				},
				properties: {
					situation_id: s.id,
					title: s.displayTitle ?? s.title,
					severity: s.severity,
					category: s.category,
					phase: s.phase ?? null,
					event_count: s.eventCount,
					source_count: s.sourceCount,
					region: s.region
				}
			}));
		// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
		(map.getSource('situations') as GeoJSONSource | undefined)?.setData({
			type: 'FeatureCollection',
			features
		});
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
			}, 'events-circle'); // Insert below event layers

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
			}, 'events-circle');

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
			}, 'events-circle');

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
		if (map?.getLayer('notam-area-fill')) map.setLayoutProperty('notam-area-fill', 'visibility', vis);
		if (map?.getLayer('notam-area-line')) map.setLayoutProperty('notam-area-line', 'visibility', vis);
	});

	// Update satellite positions on the map from the satellite store
	$effect(() => {
		if (!mapLoaded || !map?.getSource('satellite-positions')) return;
		const positions = satelliteStore.positions;
		const visible = satelliteStore.visible;
		const vis = visible ? 'visible' : 'none';
		if (map.getLayer('satellite-dots')) map.setLayoutProperty('satellite-dots', 'visibility', vis);
		if (map.getLayer('satellite-labels')) map.setLayoutProperty('satellite-labels', 'visibility', vis);
		if (!visible || positions.length === 0) return;
		const features = positions.map((sat) => ({
			type: 'Feature' as const,
			geometry: {
				type: 'Point' as const,
				coordinates: [sat.lon, sat.lat]
			},
			properties: {
				name: sat.name,
				norad_id: sat.norad_id,
				altitude_km: Math.round(sat.altitude_km)
			}
		}));
		// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
		(map.getSource('satellite-positions') as GeoJSONSource | undefined)?.setData({
			type: 'FeatureCollection',
			features
		});
	});

	// Update satellite orbit trail and future path lines
	$effect(() => {
		if (!mapLoaded) return;
		const trail = satelliteStore.orbitTrail;
		const future = satelliteStore.orbitFuture;
		const visible = satelliteStore.visible;
		const vis = visible ? 'visible' : 'none';

		for (const layerId of ['satellite-trail-line', 'satellite-future-line']) {
			if (map?.getLayer(layerId)) map.setLayoutProperty(layerId, 'visibility', vis);
		}

		// Build line segments, splitting at the antimeridian to avoid wrap-around artifacts
		const toSegments = (points: typeof trail) => {
			if (points.length < 2) return [];
			const segments: [number, number][][] = [];
			const first = points[0]!;
			let current: [number, number][] = [[first.lon, first.lat]];
			for (let i = 1; i < points.length; i++) {
				const prev = points[i - 1]!;
				const cur = points[i]!;
				// If longitude jumps > 180°, start a new segment (antimeridian crossing)
				if (Math.abs(cur.lon - prev.lon) > 180) {
					if (current.length >= 2) segments.push(current);
					current = [];
				}
				current.push([cur.lon, cur.lat]);
			}
			if (current.length >= 2) segments.push(current);
			return segments;
		};

		if (map?.getSource('satellite-trail')) {
			const segments = toSegments(trail);
			// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
			(map.getSource('satellite-trail') as GeoJSONSource | undefined)?.setData({
				type: 'FeatureCollection',
				features: segments.map((coords) => ({
					type: 'Feature' as const,
					geometry: { type: 'LineString' as const, coordinates: coords },
					properties: {}
				}))
			});
		}

		if (map?.getSource('satellite-future')) {
			const segments = toSegments(future);
			// MapLibre getSource() returns Source | undefined; cast to GeoJSONSource for setData()
			(map.getSource('satellite-future') as GeoJSONSource | undefined)?.setData({
				type: 'FeatureCollection',
				features: segments.map((coords) => ({
					type: 'Feature' as const,
					geometry: { type: 'LineString' as const, coordinates: coords },
					properties: {}
				}))
			});
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
		if (eventUpdateTimer) clearInterval(eventUpdateTimer);
		satelliteStore.stop();
		delete window.__srOpenDetail;
		delete window.__srOpenSituation;
		delete window.__srDetailProps;
		map?.remove();
	});
</script>

<div class="relative h-full w-full">
	<div class="h-full w-full" bind:this={container}></div>
	<div class="absolute bottom-2 left-1/2 z-10 w-[60%] -translate-x-1/2">
		<TimelineSlider />
	</div>
</div>
