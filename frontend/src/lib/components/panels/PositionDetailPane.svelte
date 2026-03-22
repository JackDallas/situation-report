<script lang="ts">
	import { uiStore } from '$lib/stores/ui.svelte';
	import { mapStore } from '$lib/stores/map.svelte';
	import { clockStore } from '$lib/stores/clock.svelte';
	import { formatTimestamp } from '$lib/services/event-display';
	import { getShipTypeName, getNavStatusName } from '$lib/services/vessel-types';
	import { getAircraftInfo, getAircraftCategory, decodeSquawk } from '$lib/services/aircraft-types';

	const position = $derived(uiStore.selectedPosition);

	const isVessel = $derived(
		position ? !['airplaneslive', 'adsb-lol', 'adsb-fi', 'opensky'].includes(position.source_type) &&
			!position.source_type.includes('flight') : false
	);
	const isAircraft = $derived(!isVessel);

	const payload = $derived((position?.payload ?? {}) as Record<string, unknown>);

	/** Helper to extract a string property from payload */
	function pStr(key: string): string | undefined {
		const v = payload[key];
		return typeof v === 'string' ? v : undefined;
	}
	/** Helper to extract a string-or-number property from payload */
	function pStrNum(key: string): string | number | undefined {
		const v = payload[key];
		if (typeof v === 'string' || typeof v === 'number') return v;
		return undefined;
	}

	const isMilitary = $derived(payload['military'] === true);
	const affiliation = $derived(pStr('affiliation'));

	// Aircraft derived data
	const aircraftInfo = $derived(getAircraftInfo(pStr('type_code') ?? pStr('aircraft_type')));
	const aircraftCategory = $derived(getAircraftCategory(pStr('category')));
	const squawkAlert = $derived(decodeSquawk(pStr('squawk')));
	const callsign = $derived(pStr('callsign') ?? position?.entity_name ?? position?.entity_id ?? 'Unknown');
	const registration = $derived(pStr('registration'));
	const hexCode = $derived(pStr('hex') ?? pStr('icao24') ?? position?.entity_id ?? '');

	// Vessel derived data
	const vesselName = $derived(pStr('name') ?? position?.entity_name ?? 'Unknown Vessel');
	const mmsi = $derived(pStr('mmsi') ?? position?.entity_id ?? '');
	const shipTypeName = $derived(getShipTypeName(pStrNum('ship_type')));
	const navStatus = $derived(
		payload['nav_status']
			? String(payload['nav_status'])
			: payload['nav_status_code'] != null
				? getNavStatusName(pStrNum('nav_status_code'))
				: null
	);

	// Common
	const heading = $derived(position?.heading);
	const speed = $derived(position?.speed);
	const altitude = $derived(position?.altitude);

	let showPayload = $state(false);

	function close() {
		uiStore.selectedPosition = null;
		showPayload = false;
		uiStore.openPanel('sitreps');
	}

	function flyTo() {
		if (position) {
			mapStore.flyTo(position.longitude, position.latitude);
		}
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape' && position) close();
	}

	/** Format heading as compass direction. */
	function compassDir(deg: number | null | undefined): string {
		if (deg == null) return 'N/A';
		const dirs = ['N', 'NNE', 'NE', 'ENE', 'E', 'ESE', 'SE', 'SSE', 'S', 'SSW', 'SW', 'WSW', 'W', 'WNW', 'NW', 'NNW'];
		const idx = Math.round(((deg % 360) + 360) % 360 / 22.5) % 16;
		return `${Math.round(deg)}° ${dirs[idx]}`;
	}

	/** Format ETA object to readable string. */
	function formatEta(eta: unknown): string | null {
		if (!eta) return null;
		if (typeof eta === 'string') return eta;
		if (typeof eta !== 'object') return null;
		const e = eta as Record<string, unknown>;
		const parts: string[] = [];
		if (e.Month != null && e.Day != null) parts.push(`${e.Month}/${e.Day}`);
		if (e.Hour != null && e.Minute != null) parts.push(`${String(e.Hour).padStart(2, '0')}:${String(e.Minute).padStart(2, '0')}`);
		return parts.length > 0 ? parts.join(' ') : null;
	}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if position}
	<div class="flex h-full flex-col">
		<!-- Header -->
		<div class="border-b border-border-default px-4 py-3">
			<div class="flex items-center gap-2">
				<!-- Type badge -->
				{#if isAircraft}
					<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-sky-500/15 text-sky-400">
						{#if aircraftInfo}
							{aircraftInfo.role}
						{:else}
							AIRCRAFT
						{/if}
					</span>
				{:else}
					<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-teal-500/15 text-teal-400">
						{shipTypeName}
					</span>
				{/if}

				<!-- Military / Civilian badge -->
				{#if isMilitary}
					<span class="rounded px-1.5 py-0.5 text-[10px] font-bold bg-red-500/20 text-red-400" title="Military aircraft/vessel">
						MIL
					</span>
				{:else}
					<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-gray-500/15 text-gray-400" title="Civilian aircraft/vessel">
						CIV
					</span>
				{/if}

				<!-- Affiliation flag -->
				{#if affiliation}
					<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-indigo-500/15 text-indigo-400" title="Country affiliation: {affiliation}">
						{affiliation}
					</span>
				{/if}

				<!-- Squawk emergency -->
				{#if squawkAlert?.alert}
					<span class="animate-pulse rounded px-1.5 py-0.5 text-[10px] font-bold bg-red-600/30 text-red-300" title="Squawk code emergency — {squawkAlert.text}">
						{squawkAlert.text}
					</span>
				{/if}

				<button
					onclick={close}
					class="ml-auto rounded p-1 text-text-muted hover:bg-bg-surface hover:text-text-primary"
					title="Close (Esc)"
					aria-label="Close position detail pane"
				>
					<svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12" />
					</svg>
				</button>
			</div>

			<!-- Title -->
			<div class="mt-2">
				{#if isAircraft}
					<p class="text-sm font-bold {isMilitary ? 'text-red-400' : 'text-sky-300'}">
						{callsign}
					</p>
					{#if aircraftInfo}
						<p class="text-xs text-text-secondary">{aircraftInfo.name}</p>
					{/if}
					{#if registration}
						<p class="text-[10px] text-text-muted font-mono">{registration}</p>
					{/if}
				{:else}
					<p class="text-sm font-bold {isMilitary ? 'text-red-400' : 'text-teal-300'}">
						{vesselName}
					</p>
					{#if navStatus}
						<p class="text-xs text-text-secondary">{navStatus}</p>
					{/if}
				{/if}
			</div>
		</div>

		<!-- Body -->
		<div class="flex-1 overflow-auto px-4 py-3">
			<!-- Status bar -->
			<div class="grid grid-cols-2 gap-2">
				{#if isAircraft}
					<!-- Altitude -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase">Altitude</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{#if altitude != null}
								{Math.round(altitude).toLocaleString()} ft
								{#if payload.baro_rate != null}
									<span class="text-[10px] {Number(payload.baro_rate) > 0 ? 'text-emerald-400' : Number(payload.baro_rate) < 0 ? 'text-orange-400' : 'text-text-muted'}">
										{Number(payload.baro_rate) > 0 ? '▲' : Number(payload.baro_rate) < 0 ? '▼' : ''}
										{#if payload.baro_rate != null && Number(payload.baro_rate) !== 0}
											{Math.abs(Math.round(Number(payload.baro_rate)))} fpm
										{/if}
									</span>
								{/if}
							{:else if payload.alt_baro != null}
								{Math.round(Number(payload.alt_baro)).toLocaleString()} ft
							{:else if payload.alt_geom != null}
								{Math.round(Number(payload.alt_geom)).toLocaleString()} ft
							{:else}
								N/A
							{/if}
						</div>
					</div>

					<!-- Speed -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase">Speed</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{#if speed != null}
								{Math.round(speed)} kts
							{:else if payload.ground_speed != null}
								{Math.round(Number(payload.ground_speed))} kts
							{:else if payload.velocity != null}
								{Math.round(Number(payload.velocity))} kts
							{:else}
								N/A
							{/if}
						</div>
					</div>

					<!-- Heading -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase">Heading</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{compassDir(heading ?? payload['track'] as number | null)}
						</div>
					</div>

					<!-- Squawk -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase" title="Transponder code assigned by ATC — special codes: 7500 hijack, 7600 radio failure, 7700 emergency">Squawk</div>
						<div class="text-sm font-mono font-medium {squawkAlert?.alert ? 'text-red-400' : 'text-text-primary'}">
							{payload.squawk ?? 'N/A'}
							{#if squawkAlert}
								<span class="text-[10px] {squawkAlert.alert ? 'text-red-400 font-bold' : ''}">{squawkAlert.text}</span>
							{/if}
						</div>
					</div>
				{:else}
					<!-- Vessel: Speed -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase">Speed</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{speed != null ? `${Math.round(speed * 10) / 10} kts` : 'N/A'}
						</div>
					</div>

					<!-- Vessel: Course -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase">Course</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{compassDir(payload['course'] as number | null ?? heading)}
						</div>
					</div>

					<!-- Vessel: Heading -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase">Heading</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{compassDir(heading)}
						</div>
					</div>

					<!-- Vessel: Draught -->
					<div class="rounded bg-bg-card p-2">
						<div class="text-[10px] text-text-muted uppercase" title="Vessel draft — depth below waterline in meters">Draught</div>
						<div class="text-sm font-mono font-medium text-text-primary">
							{payload.draught != null ? `${payload.draught} m` : 'N/A'}
						</div>
					</div>
				{/if}
			</div>

			<!-- Details grid -->
			<div class="mt-4 space-y-1.5">
				<span class="text-[10px] font-semibold uppercase tracking-wider text-text-muted">Details</span>

				{#if isAircraft}
					{#if hexCode}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="ICAO 24-bit aircraft transponder address (hexadecimal)">ICAO Hex</span>
							<span class="text-text-secondary font-mono">{hexCode}</span>
						</div>
					{/if}
					{#if payload.type_code ?? payload.aircraft_type}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="ICAO aircraft type designator (e.g. B738, A320)">Type Code</span>
							<span class="text-text-secondary font-mono">{payload.type_code ?? payload.aircraft_type}</span>
						</div>
					{/if}
					{#if aircraftCategory && aircraftCategory !== 'Unknown'}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Category</span>
							<span class="text-text-secondary">{aircraftCategory}</span>
						</div>
					{/if}
					{#if payload.origin_country}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Origin</span>
							<span class="text-text-secondary">{payload.origin_country}</span>
						</div>
					{/if}
					{#if payload.on_ground != null}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">On Ground</span>
							<span class="text-text-secondary">{payload.on_ground ? 'Yes' : 'No'}</span>
						</div>
					{/if}
					{#if payload.vertical_rate != null}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="Rate of climb or descent in feet per minute">Vertical Rate</span>
							<span class="text-text-secondary font-mono">{Math.round(Number(payload.vertical_rate))} fpm</span>
						</div>
					{/if}
					{#if payload.emergency && payload.emergency !== 'none'}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Emergency</span>
							<span class="text-red-400 font-medium">{payload.emergency}</span>
						</div>
					{/if}
				{:else}
					{#if mmsi}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="Maritime Mobile Service Identity — 9-digit vessel identifier">MMSI</span>
							<span class="text-text-secondary font-mono">{mmsi}</span>
						</div>
					{/if}
					{#if payload.imo}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="International Maritime Organization ship identification number">IMO</span>
							<a
								href="https://www.marinetraffic.com/en/ais/details/ships/imo:{payload.imo}"
								target="_blank"
								rel="noopener noreferrer"
								class="text-accent hover:underline font-mono"
							>
								{payload.imo}
							</a>
						</div>
					{/if}
					{#if payload.call_sign}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="Vessel radio call sign">Call Sign</span>
							<span class="text-text-secondary font-mono">{payload.call_sign}</span>
						</div>
					{/if}
					{#if payload.destination}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Destination</span>
							<span class="text-text-secondary">{payload.destination}</span>
						</div>
					{/if}
					{#if formatEta(payload.eta)}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted" title="Estimated Time of Arrival at destination">ETA</span>
							<span class="text-text-secondary">{formatEta(payload.eta)}</span>
						</div>
					{/if}
					{#if payload.length != null || payload.width != null}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Dimensions</span>
							<span class="text-text-secondary">
								{payload.length ?? '?'}m x {payload.width ?? '?'}m
							</span>
						</div>
					{/if}
					{#if payload.ship_type != null}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Ship Type</span>
							<span class="text-text-secondary">{shipTypeName} ({payload.ship_type})</span>
						</div>
					{/if}
					{#if payload.military_designation}
						<div class="flex gap-2 text-xs">
							<span class="w-24 flex-shrink-0 text-text-muted">Designation</span>
							<span class="text-red-400 font-medium">{payload.military_designation}</span>
						</div>
					{/if}
				{/if}

				<!-- Common fields -->
				{#if payload.region}
					<div class="flex gap-2 text-xs">
						<span class="w-24 flex-shrink-0 text-text-muted">Region</span>
						<span class="text-text-secondary">{payload.region}</span>
					</div>
				{/if}
				<div class="flex gap-2 text-xs">
					<span class="w-24 flex-shrink-0 text-text-muted">Source</span>
					<span class="text-text-secondary">{position.source_type}</span>
				</div>
				<div class="flex gap-2 text-xs">
					<span class="w-24 flex-shrink-0 text-text-muted">Last Seen</span>
					<span class="text-text-secondary">{formatTimestamp(position.last_seen, clockStore.now)}</span>
				</div>
			</div>

			<!-- Position + Fly to -->
			<div class="mt-4 flex items-center gap-2">
				<span class="text-xs font-mono text-text-muted">
					{position.latitude.toFixed(4)}, {position.longitude.toFixed(4)}
				</span>
				<button
					onclick={flyTo}
					class="rounded bg-bg-surface px-2 py-1 text-[10px] font-medium text-text-secondary transition-colors hover:bg-bg-card-hover hover:text-text-primary"
					title="Center map on this position"
				>
					Fly to
				</button>
			</div>

			<!-- Outlinks -->
			<div class="mt-4 flex flex-wrap gap-2">
				{#if isAircraft}
					{#if hexCode}
						<a
							href="https://globe.airplanes.live/?icao={encodeURIComponent(hexCode)}"
							target="_blank"
							rel="noopener noreferrer"
							class="inline-flex items-center gap-1.5 rounded bg-sky-500/10 px-3 py-1.5 text-xs font-medium text-sky-400 transition-colors hover:bg-sky-500/20"
						>
							ADS-B Exchange
							<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
							</svg>
						</a>
						<a
							href="https://www.flightradar24.com/{encodeURIComponent(hexCode)}"
							target="_blank"
							rel="noopener noreferrer"
							class="inline-flex items-center gap-1.5 rounded bg-sky-500/10 px-3 py-1.5 text-xs font-medium text-sky-400 transition-colors hover:bg-sky-500/20"
						>
							Flightradar24
							<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
							</svg>
						</a>
					{/if}
				{:else}
					{#if mmsi}
						<a
							href="https://www.marinetraffic.com/en/ais/details/ships/{encodeURIComponent(mmsi)}"
							target="_blank"
							rel="noopener noreferrer"
							class="inline-flex items-center gap-1.5 rounded bg-teal-500/10 px-3 py-1.5 text-xs font-medium text-teal-400 transition-colors hover:bg-teal-500/20"
						>
							MarineTraffic
							<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
							</svg>
						</a>
						<a
							href="https://www.vesselfinder.com/vessels/details/{encodeURIComponent(mmsi)}"
							target="_blank"
							rel="noopener noreferrer"
							class="inline-flex items-center gap-1.5 rounded bg-teal-500/10 px-3 py-1.5 text-xs font-medium text-teal-400 transition-colors hover:bg-teal-500/20"
						>
							VesselFinder
							<svg class="h-3 w-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
								<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 6H6a2 2 0 00-2 2v10a2 2 0 002 2h10a2 2 0 002-2v-4M14 4h6m0 0v6m0-6L10 14" />
							</svg>
						</a>
					{/if}
				{/if}
			</div>

			<!-- Raw payload toggle -->
			<div class="mt-4 border-t border-border-default pt-3">
				<button
					onclick={() => (showPayload = !showPayload)}
					class="flex items-center gap-1 text-[10px] font-medium uppercase tracking-wider text-text-muted hover:text-text-secondary"
				>
					<svg
						class="h-3 w-3 transition-transform {showPayload ? 'rotate-90' : ''}"
						fill="none"
						stroke="currentColor"
						viewBox="0 0 24 24"
					>
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 5l7 7-7 7" />
					</svg>
					Raw Payload
				</button>
				{#if showPayload}
					<pre class="mt-2 max-h-64 overflow-auto rounded bg-bg-card p-2 text-[10px] text-text-secondary">{JSON.stringify(payload, null, 2)}</pre>
				{/if}
			</div>
		</div>
	</div>
{/if}
