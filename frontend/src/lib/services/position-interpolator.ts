import { mapStore, type PositionEntry } from '$lib/stores/map.svelte';

const EARTH_RADIUS_KM = 6371;
const KTS_TO_KPH = 1.852;
const UPDATE_INTERVAL_MS = 100;

/** Affiliation color map — must match MapPanel.svelte for icon images */
const AFFILIATION_COLORS: Record<string, string> = {
	'US': '#3b82f6',
	'RU': '#ef4444',
	'CN': '#f97316',
	'IL': '#22d3ee',
	'IR': '#10b981',
	'UA': '#eab308',
	'GB': '#6366f1',
	'FR': '#8b5cf6',
	'DE': '#ec4899',
	'NATO': '#818cf8',
};
const FLIGHT_SOURCES = ['airplaneslive', 'adsb-lol', 'adsb-fi', 'opensky'];

let rafId: number | null = null;
let lastTick = 0;

/** Reference to the MapLibre map instance — set via setMapInstance() */
let mapInstance: any = null;

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

/** Build a GeoJSON feature for a position entry */
function buildPositionFeature(pos: PositionEntry, lat: number, lng: number): any {
	const affiliation = (pos.payload as any)?.affiliation ?? null;
	const isMil = (pos.payload as any)?.military === true;
	const isHighValue = (pos.payload as any)?.high_value === true;
	const isFlight =
		FLIGHT_SOURCES.includes(pos.source_type) ||
		pos.source_type.includes('flight');
	const isVessel = !isFlight;
	const onGround = pos.altitude != null && pos.altitude <= 0;

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
			on_ground: onGround
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
export function setMapInstance(map: any) {
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
	const tierFeatures: any[][] = [[], [], [], [], []];

	// Iterate base positions and extrapolate moving ones
	for (const [entityId, base] of basePositions) {
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

		const isMil = (base.payload as any)?.military === true;
		const isHighValue = (base.payload as any)?.high_value === true;
		const isFlight =
			FLIGHT_SOURCES.includes(base.source_type) ||
			base.source_type.includes('flight');
		const isVessel = !isFlight;

		// Zoom-based filtering: skip civilian flights at low zoom
		if (zoom < 4 && !isMil && !isHighValue && !isVessel) continue;
		if (zoom < 3 && !isMil && !isHighValue) continue;

		const feature = buildPositionFeature(base, lat, lng);

		// Assign to priority tier
		if (isHighValue) tierFeatures[0].push(feature);
		else if (isMil && feature.properties.affiliation) tierFeatures[1].push(feature);
		else if (isMil) tierFeatures[2].push(feature);
		else if (isVessel) tierFeatures[3].push(feature);
		else tierFeatures[4].push(feature);
	}

	// Cap total positions by zoom level to prevent map overload
	const maxPositions = zoom < 4 ? 500 : zoom < 6 ? 1000 : 3000;
	const features: any[] = [];
	for (const tier of tierFeatures) {
		for (const f of tier) {
			if (features.length >= maxPositions) break;
			features.push(f);
		}
		if (features.length >= maxPositions) break;
	}

	// Update MapLibre directly — bypass reactive store to avoid $effect overhead
	(mapInstance.getSource('positions') as any).setData({
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
