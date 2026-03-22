import { mapStore, type PositionEntry } from '$lib/stores/map.svelte';
import { AFFILIATION_COLORS } from '$lib/config/colors';
import type { Map as MapLibreMap, GeoJSONSource } from 'maplibre-gl';

const EARTH_RADIUS_KM = 6371;
const KTS_TO_KPH = 1.852;
const UPDATE_INTERVAL_MS = 100;
const FLIGHT_SOURCES = ['airplaneslive', 'adsb-lol', 'adsb-fi', 'opensky'];

let rafId: number | null = null;
let lastTick = 0;

/** Reference to the MapLibre map instance — set via setMapInstance() */
let mapInstance: MapLibreMap | null = null;

/**
 * Extrapolate a position forward using heading (degrees) and speed (knots).
 * Returns new [lng, lat].
 */
function extrapolate(
	lat: number,
	lng: number,
	headingDeg: number,
	speedKnots: number,
	dtSeconds: number
): [number, number] {
	const speedKph = speedKnots * KTS_TO_KPH;
	const distKm = (speedKph / 3600) * dtSeconds;
	const bearingRad = (headingDeg * Math.PI) / 180;
	const latRad = (lat * Math.PI) / 180;
	const lngRad = (lng * Math.PI) / 180;
	const angularDist = distKm / EARTH_RADIUS_KM;

	const newLatRad = Math.asin(
		Math.sin(latRad) * Math.cos(angularDist) +
			Math.cos(latRad) * Math.sin(angularDist) * Math.cos(bearingRad)
	);
	const newLngRad =
		lngRad +
		Math.atan2(
			Math.sin(bearingRad) * Math.sin(angularDist) * Math.cos(latRad),
			Math.cos(angularDist) - Math.sin(latRad) * Math.sin(newLatRad)
		);

	return [(newLngRad * 180) / Math.PI, (newLatRad * 180) / Math.PI];
}

/** Position feature for the MapLibre GeoJSON source */
interface PositionFeature {
	type: 'Feature';
	geometry: { type: 'Point'; coordinates: [number, number] };
	properties: {
		entity_id: string;
		label: string;
		heading: number | null;
		speed: number | null;
		altitude: number | null;
		source_type: string;
		pos_type: string;
		icon_image: string;
		affiliation: string | null;
		is_military: boolean;
		is_high_value: boolean;
		is_vessel: boolean;
		on_ground: boolean;
		is_stale: boolean;
	};
}

/** Build a GeoJSON feature for a position entry */
function buildPositionFeature(pos: PositionEntry, lat: number, lng: number): PositionFeature {
	const affiliationRaw = pos.payload['affiliation'];
	const affiliation = typeof affiliationRaw === 'string' ? affiliationRaw : null;
	const isMil = pos.payload['military'] === true;
	const isHighValue = pos.payload['high_value'] === true;
	const isFlight =
		FLIGHT_SOURCES.includes(pos.source_type) ||
		pos.source_type.includes('flight');
	const isVessel = !isFlight;
	const onGround = pos.altitude != null && pos.altitude <= 0;
	const isStale = isFlight && pos.heading == null && pos.speed == null;

	let posType: string;
	let iconImage: string;
	if (isMil && affiliation && AFFILIATION_COLORS[affiliation]) {
		posType = `mil-${affiliation}`;
		iconImage = `arrow-mil-${affiliation}`;
	} else if (isMil) {
		posType = 'flight-mil';
		iconImage = 'arrow-flight-mil';
	} else if (isFlight) {
		posType = 'flight';
		iconImage = 'arrow-flight';
	} else {
		posType = 'vessel';
		iconImage = 'arrow-vessel';
	}

	return {
		type: 'Feature',
		geometry: {
			type: 'Point',
			coordinates: [lng, lat]
		},
		properties: {
			entity_id: pos.entity_id,
			label: pos.entity_name || pos.entity_id,
			heading: pos.heading,
			speed: pos.speed,
			altitude: pos.altitude,
			source_type: pos.source_type,
			pos_type: posType,
			icon_image: iconImage,
			affiliation: affiliation,
			is_military: isMil,
			is_high_value: isHighValue,
			is_vessel: isVessel,
			on_ground: onGround,
			is_stale: isStale
		}
	};
}

/** Snapshot of base positions for interpolation */
let basePositions: Map<string, PositionEntry> = new Map();
let baseTime = 0;

/**
 * Called when new positions arrive from a poll.
 * Sets the base for the next interpolation cycle.
 */
export function setBasePositions(positions: Map<string, PositionEntry>) {
	basePositions = new Map(positions);
	baseTime = Date.now();
}

/**
 * Set the MapLibre map instance for direct source updates.
 * Called from MapPanel.svelte after map loads.
 */
export function setMapInstance(map: MapLibreMap) {
	mapInstance = map;
}

function tick() {
	if (document.hidden) {
		rafId = requestAnimationFrame(tick);
		return;
	}

	const now = Date.now();
	if (now - lastTick < UPDATE_INTERVAL_MS) {
		rafId = requestAnimationFrame(tick);
		return;
	}
	lastTick = now;

	if (basePositions.size === 0) {
		rafId = requestAnimationFrame(tick);
		return;
	}

	// If no map instance yet, skip — positions will be rendered on the first poll via $effect
	if (!mapInstance?.getSource('positions')) {
		rafId = requestAnimationFrame(tick);
		return;
	}

	const dtSeconds = (now - baseTime) / 1000;
	// Only interpolate for reasonable time deltas (< 60s)
	if (dtSeconds > 60 || dtSeconds < 0.1) {
		rafId = requestAnimationFrame(tick);
		return;
	}

	const hideGround = mapStore.hideGroundPlanes;
	const zoom = mapInstance.getZoom();

	// Priority tiers: 0=high_value mil, 1=known-affiliation mil, 2=unknown mil, 3=vessel, 4=civilian
	const highValue: PositionFeature[] = [];
	const knownMil: PositionFeature[] = [];
	const unknownMil: PositionFeature[] = [];
	const vessels: PositionFeature[] = [];
	const civilian: PositionFeature[] = [];
	const tierFeatures = [highValue, knownMil, unknownMil, vessels, civilian];

	// Iterate base positions and extrapolate moving ones
	for (const [_entityId, base] of basePositions) {
		let lat = base.latitude;
		let lng = base.longitude;

		// Extrapolate moving entities
		if (base.heading != null && base.speed != null && base.speed >= 1) {
			const [newLng, newLat] = extrapolate(
				base.latitude,
				base.longitude,
				base.heading,
				base.speed,
				dtSeconds
			);
			lat = newLat;
			lng = newLng;
		}

		const onGround = base.altitude != null && base.altitude <= 0;
		if (hideGround && onGround) continue;

		const isMil = base.payload['military'] === true;
		const isHighValue = base.payload['high_value'] === true;
		const isFlight =
			FLIGHT_SOURCES.includes(base.source_type) ||
			base.source_type.includes('flight');
		const isVessel = !isFlight;

		// Zoom-based filtering: skip civilian flights at low zoom
		if (zoom < 4 && !isMil && !isHighValue && !isVessel) continue;
		if (zoom < 3 && !isMil && !isHighValue) continue;

		const feature = buildPositionFeature(base, lat, lng);

		// Assign to priority tier
		if (isHighValue) highValue.push(feature);
		else if (isMil && feature.properties.affiliation) knownMil.push(feature);
		else if (isMil) unknownMil.push(feature);
		else if (isVessel) vessels.push(feature);
		else civilian.push(feature);
	}

	// Cap total positions by zoom level to prevent map overload
	const maxPositions = zoom < 4 ? 500 : zoom < 6 ? 1000 : 3000;
	const features: PositionFeature[] = [];
	for (const tier of tierFeatures) {
		for (const f of tier) {
			if (features.length >= maxPositions) break;
			features.push(f);
		}
		if (features.length >= maxPositions) break;
	}

	// Update MapLibre directly — bypass reactive store to avoid $effect overhead
	// MapLibre's getSource() returns Source | undefined; cast to GeoJSONSource for setData()
	(mapInstance.getSource('positions') as GeoJSONSource | undefined)?.setData({
		type: 'FeatureCollection',
		features
	});

	rafId = requestAnimationFrame(tick);
}

export function startInterpolation() {
	if (rafId != null) return;
	lastTick = Date.now();
	rafId = requestAnimationFrame(tick);
}

export function stopInterpolation() {
	if (rafId != null) {
		cancelAnimationFrame(rafId);
		rafId = null;
	}
	mapInstance = null;
}
