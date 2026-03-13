import type { SituationEvent, EventType, Severity } from '$lib/types/events';

export interface TypeColor {
	border: string;
	bg: string;
	text: string;
	label: string;
}

export interface SeverityColor {
	border: string;
	bg: string;
	text: string;
	badge: string;
}

export const typeColorMap: Record<EventType, TypeColor> = {
	conflict_event: { border: 'border-l-alert', bg: 'bg-alert/10', text: 'text-alert', label: 'Conflict' },
	thermal_anomaly: { border: 'border-l-warning', bg: 'bg-warning/10', text: 'text-warning', label: 'Thermal' },
	shodan_banner: { border: 'border-l-accent', bg: 'bg-accent/10', text: 'text-accent', label: 'Shodan' },
	internet_outage: { border: 'border-l-purple-500', bg: 'bg-purple-500/10', text: 'text-purple-400', label: 'Outage' },
	threat_intel: { border: 'border-l-cyan-500', bg: 'bg-cyan-500/10', text: 'text-cyan-400', label: 'Threat Intel' },
	cert_issued: { border: 'border-l-cyan-500', bg: 'bg-cyan-500/10', text: 'text-cyan-400', label: 'Certificate' },
	bgp_anomaly: { border: 'border-l-cyan-500', bg: 'bg-cyan-500/10', text: 'text-cyan-400', label: 'BGP' },
	censorship_event: { border: 'border-l-purple-500', bg: 'bg-purple-500/10', text: 'text-purple-400', label: 'Censorship' },
	flight_position: { border: 'border-l-sky-500', bg: 'bg-sky-500/10', text: 'text-sky-400', label: 'Flight' },
	vessel_position: { border: 'border-l-teal-500', bg: 'bg-teal-500/10', text: 'text-teal-400', label: 'Vessel' },
	news_article: { border: 'border-l-gray-500', bg: 'bg-gray-500/10', text: 'text-gray-400', label: 'News' },
	geo_event: { border: 'border-l-emerald-500', bg: 'bg-emerald-500/10', text: 'text-emerald-400', label: 'Geo' },
	seismic_event: { border: 'border-l-amber-500', bg: 'bg-amber-500/10', text: 'text-amber-400', label: 'Seismic' },
	nuclear_event: { border: 'border-l-yellow-400', bg: 'bg-yellow-400/10', text: 'text-yellow-300', label: 'Nuclear' },
	notam_event: { border: 'border-l-orange-500', bg: 'bg-orange-500/10', text: 'text-orange-400', label: 'NOTAM' },
	telegram_message: { border: 'border-l-blue-500', bg: 'bg-blue-500/10', text: 'text-blue-400', label: 'Telegram' },
	bgp_leak: { border: 'border-l-red-500', bg: 'bg-red-500/10', text: 'text-red-400', label: 'BGP Leak' },
	fishing_event: { border: 'border-l-teal-400', bg: 'bg-teal-400/10', text: 'text-teal-300', label: 'Fishing' },
	gps_interference: { border: 'border-l-red-400', bg: 'bg-red-400/10', text: 'text-red-300', label: 'GPS Jam' },
	source_health: { border: 'border-l-text-muted', bg: 'bg-text-muted/10', text: 'text-text-muted', label: 'Health' },
	geo_news: { border: 'border-l-emerald-400', bg: 'bg-emerald-400/10', text: 'text-emerald-300', label: 'Geo News' },
	shodan_count: { border: 'border-l-accent', bg: 'bg-accent/10', text: 'text-accent', label: 'Shodan' },
};

export const severityColors: Record<Severity, SeverityColor> = {
	info: { border: 'border-l-gray-400', bg: 'bg-gray-400/10', text: 'text-gray-300', badge: 'bg-gray-400/20 text-gray-300' },
	critical: { border: 'border-l-red-500', bg: 'bg-red-500/10', text: 'text-red-400', badge: 'bg-red-500/20 text-red-400' },
	high: { border: 'border-l-orange-500', bg: 'bg-orange-500/10', text: 'text-orange-400', badge: 'bg-orange-500/20 text-orange-400' },
	medium: { border: 'border-l-yellow-500', bg: 'bg-yellow-500/10', text: 'text-yellow-400', badge: 'bg-yellow-500/20 text-yellow-400' },
	low: { border: 'border-l-blue-500', bg: 'bg-blue-500/10', text: 'text-blue-400', badge: 'bg-blue-500/20 text-blue-400' },
};

export const defaultColor: TypeColor = { border: 'border-l-text-muted', bg: 'bg-text-muted/10', text: 'text-text-muted', label: 'Event' };
export const defaultSeverity: SeverityColor = { border: 'border-l-text-muted', bg: 'bg-text-muted/10', text: 'text-text-muted', badge: 'bg-text-muted/20 text-text-muted' };

export function getTypeColor(type: EventType | string): TypeColor {
	return typeColorMap[type as EventType] ?? defaultColor;
}

export function getSeverityColor(severity: Severity | string): SeverityColor {
	return severityColors[severity as Severity] ?? defaultSeverity;
}

export function getEventSummary(event: SituationEvent): string {
	const d = event.payload ?? {};
	switch (event.event_type) {
		case 'conflict_event':
			return `${d.event_type ?? 'Conflict'} in ${d.location ?? d.country ?? 'unknown location'}${Number(d.fatalities) > 0 ? ` - ${d.fatalities} killed` : ''}`;
		case 'thermal_anomaly':
			return `Thermal anomaly detected${d.confidence ? ` (${d.confidence} confidence)` : ''}${d.country ? ` in ${d.country}` : ''}`;
		case 'shodan_banner':
			return `${d.ip ?? '?'}:${d.port ?? '?'}${d.product ? ` - ${d.product}` : ''}${d.org ? ` (${d.org})` : ''}`;
		case 'internet_outage':
			return `${d.severity ?? ''} outage in ${d.country ?? 'unknown'}${d.outage_type ? ` (${d.outage_type})` : ''}`;
		case 'threat_intel':
			return `${d.pulse_name ?? d.name ?? 'Threat pulse'}${d.adversary ? ` [${d.adversary}]` : ''}`;
		case 'cert_issued':
			return `New cert: ${d.domain ?? d.common_name ?? 'unknown domain'}${d.issuer ? ` (${d.issuer})` : ''}`;
		case 'bgp_anomaly':
			return `BGP ${d.type ?? d.anomaly_type ?? 'anomaly'}: ${d.prefix ?? 'unknown'}${d.asn ? ` (AS${d.asn})` : ''}`;
		case 'censorship_event':
			return `Censorship in ${d.country ?? 'unknown'}${d.test_name ? ` (${d.test_name})` : ''}`;
		case 'flight_position':
			return `${d.callsign ?? 'Unknown'} - ${d.origin_country ?? ''}${d.baro_altitude ? ` at ${Number(d.baro_altitude).toLocaleString()} ft` : ''}`;
		case 'vessel_position':
			return `${d.vessel_name ?? d.mmsi ?? 'Unknown vessel'}${d.ship_type ? ` (${d.ship_type})` : ''}`;
		case 'news_article':
			return (d.title as string) ?? 'Untitled article';
		case 'geo_event':
			return event.title ?? `${d.event_type ?? 'Geo event'} in ${d.conflict ?? d.location ?? 'unknown'}`;
		case 'source_health':
			return `${event.source_type}: ${(event.payload?.status as string) ?? 'status update'}`;
		case 'seismic_event':
			return `M${d.magnitude ?? '?'} earthquake${d.place ? ` near ${d.place}` : ''}`;
		case 'nuclear_event':
			return `${d.facility_name ?? d.event_type ?? 'Nuclear event'}${d.country ? ` in ${d.country}` : ''}`;
		case 'notam_event':
			return d.qcode_description
				? `${d.qcode_category}: ${d.qcode_description}${d.location ? ` at ${d.location}` : ''}`
				: `${d.type ?? 'NOTAM'}${d.location ? ` at ${d.location}` : ''}${d.purpose ? ` (${d.purpose})` : ''}`;
		case 'telegram_message':
			return `${d.channel ?? 'Channel'}: ${(d.text as string)?.slice(0, 80) ?? 'message'}`;
		case 'gps_interference':
			return `GPS interference${d.region ? ` in ${d.region}` : ''}${d.level ? ` (level ${d.level})` : ''}`;
		case 'geo_news': {
			// geo_news payload is a GeoJSON Feature: { properties: { name, html, count }, geometry, type }
			const geoProps = (d.properties as Record<string, unknown>) ?? {};
			const name = (geoProps.name as string) ?? event.entity_name ?? event.title;
			return name ?? 'Geo news';
		}
		case 'shodan_count':
			return `Shodan: ${d.query ?? 'scan'}${d.count ? ` (${d.count} results)` : ''}`;
		default:
			return `${event.source_type}: ${event.event_type} event`;
	}
}

/** Relative time string. Pass `now` from clockStore.now for reactive updates. */
export function formatTimestamp(ts: string | undefined, now?: number): string {
	if (!ts) return 'now';
	try {
		const date = new Date(ts);
		const diffMs = (now ?? Date.now()) - date.getTime();
		const diffSec = Math.floor(diffMs / 1000);
		if (diffSec < 0) return 'just now';
		if (diffSec < 10) return 'just now';
		if (diffSec < 60) return `${diffSec}s ago`;
		const diffMin = Math.floor(diffSec / 60);
		if (diffMin < 60) return `${diffMin}m ago`;
		const diffH = Math.floor(diffMin / 60);
		if (diffH < 24) return `${diffH}h ago`;
		const diffD = Math.floor(diffH / 24);
		if (diffD < 7) return `${diffD}d ago`;
		return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
	} catch {
		return '';
	}
}

/** Compact absolute time: "14:32" or "14:32 Jan 25" if older than today. */
export function formatAbsoluteTime(ts: string | undefined, now?: number): string {
	if (!ts) return '';
	try {
		const date = new Date(ts);
		const today = new Date(now ?? Date.now());
		const time = date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
		if (
			date.getDate() === today.getDate() &&
			date.getMonth() === today.getMonth() &&
			date.getFullYear() === today.getFullYear()
		) {
			return time;
		}
		return `${time} ${date.toLocaleDateString([], { month: 'short', day: 'numeric' })}`;
	} catch {
		return '';
	}
}

/** Full tooltip timestamp: "2026-03-01 14:32:05 UTC" */
export function formatFullTimestamp(ts: string | undefined): string {
	if (!ts) return '';
	try {
		const date = new Date(ts);
		return date.toISOString().replace('T', ' ').replace(/\.\d+Z$/, ' UTC');
	} catch {
		return '';
	}
}
