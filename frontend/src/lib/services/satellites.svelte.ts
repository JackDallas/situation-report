/**
 * Satellite position tracking service.
 *
 * Fetches TLE data from the backend API and uses satellite.js to propagate
 * positions client-side in real time. Provides a reactive Svelte 5 store
 * with current positions for the 3 FIRMS satellites (Suomi NPP, NOAA-20, NOAA-21).
 */
import {
	twoline2satrec,
	propagate,
	gstime,
	eciToGeodetic,
	degreesLong,
	degreesLat,
	type SatRec,
} from 'satellite.js';

export interface SatellitePosition {
	name: string;
	norad_id: number;
	lat: number;
	lon: number;
	altitude_km: number;
}

export interface OrbitPoint {
	lat: number;
	lon: number;
	altitude_km: number;
	time: Date;
}

interface TleData {
	name: string;
	norad_id: number;
	tle_line1: string;
	tle_line2: string;
}

interface SatRecord {
	name: string;
	norad_id: number;
	satrec: SatRec;
}

class SatelliteStore {
	positions = $state<SatellitePosition[]>([]);
	visible = $state(true);
	selectedNoradId = $state<number | null>(null);
	orbitTrail = $state<OrbitPoint[]>([]);
	orbitFuture = $state<OrbitPoint[]>([]);
	private satRecords: SatRecord[] = [];
	private propagateTimer: ReturnType<typeof setInterval> | null = null;
	private refreshTimer: ReturnType<typeof setInterval> | null = null;
	private started = false;

	toggle() {
		this.visible = !this.visible;
	}

	/** Initialize the satellite tracking service. Call once from the app. */
	start() {
		if (this.started) return;
		this.started = true;

		// Fetch TLEs immediately
		this.fetchTles();

		// Re-fetch TLEs from backend every 30 minutes
		this.refreshTimer = setInterval(() => this.fetchTles(), 30 * 60 * 1000);

		// Propagate positions every 3 seconds
		this.propagateTimer = setInterval(() => this.propagatePositions(), 3000);
	}

	/** Stop all timers. */
	stop() {
		if (this.propagateTimer) {
			clearInterval(this.propagateTimer);
			this.propagateTimer = null;
		}
		if (this.refreshTimer) {
			clearInterval(this.refreshTimer);
			this.refreshTimer = null;
		}
		this.started = false;
	}

	/** Fetch TLE data from the backend API and parse into satellite records. */
	private async fetchTles() {
		try {
			const resp = await fetch('/api/satellite-tles');
			if (!resp.ok) {
				console.warn('Failed to fetch satellite TLEs:', resp.status);
				return;
			}
			const tles: TleData[] = await resp.json();
			this.satRecords = tles
				.map((tle) => {
					try {
						const satrec = twoline2satrec(tle.tle_line1, tle.tle_line2);
						return { name: tle.name, norad_id: tle.norad_id, satrec };
					} catch (e) {
						console.warn(`Failed to parse TLE for ${tle.name}:`, e);
						return null;
					}
				})
				.filter((r): r is SatRecord => r !== null);

			// Propagate immediately after fetching new TLEs
			this.propagatePositions();
		} catch (e) {
			console.warn('Satellite TLE fetch error:', e);
		}
	}

	/** Propagate all satellite positions to the current time. */
	private propagatePositions() {
		if (this.satRecords.length === 0) return;

		const now = new Date();
		const gmst = gstime(now);
		const newPositions: SatellitePosition[] = [];

		for (const sat of this.satRecords) {
			try {
				const posVel = propagate(sat.satrec, now);
				if (!posVel || !posVel.position) {
					continue;
				}
				const geodetic = eciToGeodetic(posVel.position, gmst);
				newPositions.push({
					name: sat.name,
					norad_id: sat.norad_id,
					lat: degreesLat(geodetic.latitude),
					lon: degreesLong(geodetic.longitude),
					altitude_km: geodetic.height,
				});
			} catch {
				// Propagation can fail for stale TLEs — skip silently
			}
		}

		this.positions = newPositions;

		// Re-compute orbit path for selected satellite
		if (this.selectedNoradId != null) {
			this.computeOrbitPath(this.selectedNoradId);
		}
	}

	/** Select a satellite and compute its orbit path. Pass null to deselect. */
	selectSatellite(noradId: number | null) {
		if (this.selectedNoradId === noradId) {
			// Toggle off
			this.selectedNoradId = null;
			this.orbitTrail = [];
			this.orbitFuture = [];
			return;
		}
		this.selectedNoradId = noradId;
		if (noradId != null) {
			this.computeOrbitPath(noradId);
		} else {
			this.orbitTrail = [];
			this.orbitFuture = [];
		}
	}

	/** Propagate orbit path: 45 min past (trail) + 45 min future, sampled every 30s. */
	private computeOrbitPath(noradId: number) {
		const sat = this.satRecords.find((s) => s.norad_id === noradId);
		if (!sat) return;

		const now = Date.now();
		const PAST_MS = 45 * 60 * 1000;
		const FUTURE_MS = 45 * 60 * 1000;
		const STEP_MS = 30 * 1000; // 30-second intervals

		const trail: OrbitPoint[] = [];
		const future: OrbitPoint[] = [];

		// Past trail (oldest → now)
		for (let t = now - PAST_MS; t <= now; t += STEP_MS) {
			const pt = this.propagateAt(sat.satrec, new Date(t));
			if (pt) trail.push(pt);
		}

		// Future path (now → future)
		for (let t = now; t <= now + FUTURE_MS; t += STEP_MS) {
			const pt = this.propagateAt(sat.satrec, new Date(t));
			if (pt) future.push(pt);
		}

		this.orbitTrail = trail;
		this.orbitFuture = future;
	}

	/** Propagate a single satellite to a specific time. Returns null on failure. */
	private propagateAt(satrec: SatRec, time: Date): OrbitPoint | null {
		try {
			const posVel = propagate(satrec, time);
			if (!posVel || !posVel.position) return null;
			const gmst = gstime(time);
			const geodetic = eciToGeodetic(posVel.position, gmst);
			return {
				lat: degreesLat(geodetic.latitude),
				lon: degreesLong(geodetic.longitude),
				altitude_km: geodetic.height,
				time,
			};
		} catch {
			return null;
		}
	}
}

export const satelliteStore = new SatelliteStore();
