import type { SituationEvent } from '$lib/types/events';

export interface Outlink {
	url: string;
	label: string;
}

export function getOutlink(event: SituationEvent): Outlink | null {
	const p = event.payload ?? {};
	const eid = event.entity_id ?? '';
	const sid = event.source_id ?? '';

	switch (event.source_type as string) {
		case 'shodan':
		case 'shodan-discovery':
		case 'shodan-search': {
			const ip = (p.ip as string) ?? eid;
			return ip ? { url: `https://www.shodan.io/host/${encodeURIComponent(ip)}`, label: 'Shodan' } : null;
		}
		case 'otx':
			return sid ? { url: `https://otx.alienvault.com/pulse/${encodeURIComponent(sid)}`, label: 'OTX' } : null;
		case 'certstream':
			return eid ? { url: `https://crt.sh/?q=${encodeURIComponent(eid)}`, label: 'crt.sh' } : null;
		case 'gdelt':
		case 'rss-news': {
			const articleUrl = (p.url as string) ?? (p.source_url as string) ?? (p.link as string);
			return articleUrl ? { url: articleUrl, label: 'Article' } : null;
		}
		case 'gdelt-geo': {
			const geoProps = (p.properties as Record<string, unknown>) ?? {};
			const htmlContent = (geoProps.html as string) ?? '';
			const hrefMatch = htmlContent.match(/href="([^"]+)"/);
			const articleUrl = hrefMatch?.[1] ?? (p.url as string | undefined);
			return articleUrl ? { url: articleUrl, label: 'Article' } : null;
		}
		case 'airplaneslive':
		case 'adsb-lol':
		case 'adsb-fi':
		case 'opensky':
			return eid ? { url: `https://globe.airplanes.live/?icao=${encodeURIComponent(eid)}`, label: 'AirplanesLive' } : null;
		case 'ais':
		case 'gfw':
			return eid ? { url: `https://www.marinetraffic.com/en/ais/details/ships/${encodeURIComponent(eid)}`, label: 'MarineTraffic' } : null;
		case 'usgs':
			return sid ? { url: `https://earthquake.usgs.gov/earthquakes/eventpage/${encodeURIComponent(sid)}`, label: 'USGS' } : null;
		case 'ooni':
			return sid ? { url: `https://explorer.ooni.org/measurement/${encodeURIComponent(sid)}`, label: 'OONI' } : null;
		case 'ioda': {
			const country = (p.country_code as string) ?? (p.country as string);
			return country
				? { url: `https://ioda.inetintel.cc.gatech.edu/country/${encodeURIComponent(country)}`, label: 'IODA' }
				: { url: 'https://ioda.inetintel.cc.gatech.edu/', label: 'IODA' };
		}
		case 'bgp': {
			const asn = eid?.replace(/^AS/i, '');
			return asn ? { url: `https://bgptools.com/as/${encodeURIComponent(asn)}`, label: 'BGP Tools' } : null;
		}
		case 'cloudflare':
			return { url: 'https://radar.cloudflare.com/', label: 'Cloudflare Radar' };
		case 'cloudflare-bgp': {
			const originAsn = String(p.origin_asn ?? '').replace(/^AS/i, '');
			return originAsn ? { url: `https://bgptools.com/as/${encodeURIComponent(originAsn)}`, label: 'BGP Tools' } : null;
		}
		case 'geoconfirmed': {
			const gcUrl = (p.url as string) ?? (p.source_url as string);
			return gcUrl ? { url: gcUrl, label: 'GeoConfirmed' } : null;
		}
		case 'telegram': {
			const channel = (p.channel as string) ?? (p.channel_username as string);
			const msgId = (p.message_id as string | number);
			if (channel && msgId) {
				return { url: `https://t.me/${encodeURIComponent(channel)}/${msgId}`, label: 'Telegram' };
			}
			return channel ? { url: `https://t.me/${encodeURIComponent(channel)}`, label: 'Telegram' } : null;
		}
		case 'notam':
			return { url: 'https://pibs.nats.co.uk/operational/pibs/PIB.xml', label: 'NOTAM PIB' };
		case 'gpsjam':
			return { url: 'https://gpsjam.org/', label: 'GPSJam' };
		case 'nuclear':
			return { url: 'https://www.iaea.org/resources/databases/power-reactor-information-system-pris', label: 'IAEA PRIS' };
		case 'acled':
			return sid ? { url: `https://acleddata.com/data-export-tool/`, label: 'ACLED' } : null;
		case 'firms':
			return { url: 'https://firms.modaps.eosdis.nasa.gov/map/', label: 'FIRMS Map' };
		default: {
			// Generic fallback: check for URL in payload
			const fallbackUrl = (p.url as string) ?? (p.source_url as string) ?? (p.link as string);
			return fallbackUrl ? { url: fallbackUrl, label: 'Source' } : null;
		}
	}
}

export interface DetailField {
	label: string;
	value: string;
}

export function getEventDetails(event: SituationEvent): DetailField[] {
	const p = event.payload ?? {};
	const fields: DetailField[] = [];

	function add(label: string, value: unknown) {
		if (value != null && value !== '' && value !== undefined) {
			fields.push({ label, value: String(value) });
		}
	}

	switch (event.event_type as string) {
		case 'conflict_event':
			add('Type', p.event_type);
			add('Actors', [p.actor1, p.actor2].filter(Boolean).join(' vs '));
			add('Fatalities', p.fatalities);
			add('Location', p.location ?? p.country);
			add('Source', p.source);
			break;
		case 'thermal_anomaly':
			add('Satellite', p.satellite);
			add('Confidence', p.confidence);
			add('FRP', p.frp ? `${Number(p.frp).toFixed(1)} MW` : undefined);
			add('Brightness', p.bright_ti4 ? `${Number(p.bright_ti4).toFixed(1)} K` : undefined);
			add('Day/Night', p.daynight);
			add('Acq Time', p.acq_date && p.acq_time ? `${p.acq_date} ${p.acq_time} UTC` : undefined);
			break;
		case 'seismic_event':
			add('Magnitude', p.magnitude);
			add('Depth', p.depth ? `${p.depth} km` : undefined);
			add('Place', p.place);
			add('Alert Level', p.alert);
			add('Tsunami', p.tsunami ? 'Yes' : undefined);
			break;
		case 'shodan_banner':
			add('IP', p.ip);
			add('Port', p.port);
			add('Product', p.product);
			add('Organization', p.org);
			add('OS', p.os);
			break;
		case 'internet_outage':
			add('Severity', p.severity);
			add('Country', p.country);
			add('Type', p.outage_type);
			add('ASN', p.asn);
			add('Source', p.datasource);
			break;
		case 'threat_intel':
			add('Pulse', p.pulse_name ?? p.name);
			add('Adversary', p.adversary);
			add('TLP', p.tlp);
			add('Indicators', Array.isArray(p.indicators) ? `${p.indicators.length} IoCs` : undefined);
			add('Tags', Array.isArray(p.tags) ? (p.tags as string[]).slice(0, 5).join(', ') : undefined);
			break;
		case 'cert_issued':
			add('Domain', p.domain ?? p.common_name);
			add('Issuer', p.issuer);
			add('SAN Count', p.san_count);
			add('Type', p.cert_type);
			break;
		case 'bgp_anomaly':
			add('Type', p.type ?? p.anomaly_type);
			add('Prefix', p.prefix);
			add('ASN', p.asn);
			add('Peer Count', p.peer_count);
			break;
		case 'bgp_leak':
			add('Leaked Prefix', p.prefix);
			add('Origin AS', p.origin_asn);
			add('Leaker AS', p.leaker_asn);
			add('Affected', p.affected_countries);
			break;
		case 'censorship_event':
			add('Country', p.country);
			add('Test', p.test_name);
			add('Input', p.input);
			add('Blocking Type', p.blocking_type);
			break;
		case 'flight_position':
			add('Callsign', p.callsign);
			add('Altitude', p.baro_altitude ? `${Number(p.baro_altitude).toLocaleString()} ft` : undefined);
			add('Speed', p.velocity ? `${Math.round(Number(p.velocity))} kts` : undefined);
			add('Country', p.origin_country);
			add('Type', p.aircraft_type ?? p.category);
			break;
		case 'vessel_position':
			add('Name', p.vessel_name);
			add('MMSI', p.mmsi);
			add('Ship Type', p.ship_type);
			add('Speed', p.speed ? `${p.speed} kts` : undefined);
			add('Flag', p.flag);
			break;
		case 'fishing_event':
			add('Vessel', p.vessel_name);
			add('Flag', p.flag);
			add('MMSI', p.mmsi);
			add('Hours', p.fishing_hours);
			break;
		case 'news_article':
			add('Source', p.source_name ?? p.domain);
			add('Theme', p.theme);
			add('Tone', p.tone);
			add('Language', p.language);
			break;
		case 'nuclear_event':
			add('Facility', p.facility_name);
			add('Type', p.event_type);
			add('Country', p.country);
			add('Status', p.status);
			break;
		case 'notam_event':
			add('Category', p.qcode_category ?? p.type);
			add('Description', p.qcode_description);
			add('Location', p.location);
			add('FIR', p.fir_label ?? p.fir);
			add('Q-Code', p.qcode);
			add('Routine', p.is_routine === true ? 'Yes' : p.is_routine === false ? 'No' : undefined);
			add('Effective', p.start_validity ?? p.effective_start);
			add('Expires', p.end_validity);
			break;
		case 'telegram_message':
			add('Channel', p.channel);
			add('Author', p.author);
			add('Views', p.views);
			break;
		case 'gps_interference':
			add('Region', p.region);
			add('Level', p.level);
			add('Affected Flights', p.affected_flights);
			add('Source', p.source);
			break;
		case 'economic_event':
			add('Indicator', p.indicator ?? p.series_id);
			add('Value', p.value);
			add('Unit', p.unit);
			add('Period', p.period);
			break;
		case 'geo_event':
			add('Type', p.event_type);
			add('Location', p.location);
			add('Source', p.source);
			break;
		case 'geo_news': {
			// payload is nested GeoJSON: { properties: { name, html, count }, geometry, type }
			const geoProps = (p.properties as Record<string, unknown>) ?? {};
			add('Location', geoProps.name);
			add('Articles', geoProps.count);
			// Extract article titles from HTML
			const htmlStr = (geoProps.html as string) ?? '';
			const titles = [...htmlStr.matchAll(/title="([^"]+)"/g)].map(m => m[1]);
			if (titles.length > 0) add('Headlines', titles.slice(0, 3).join(' | '));
			break;
		}
		default:
			// Show first few payload keys for unknown types
			for (const [k, v] of Object.entries(p).slice(0, 4)) {
				if (v != null && v !== '') {
					add(k.replace(/_/g, ' '), typeof v === 'object' ? JSON.stringify(v).slice(0, 80) : v);
				}
			}
	}

	return fields;
}

export function escapeHtml(str: string): string {
	return str
		.replace(/&/g, '&amp;')
		.replace(/</g, '&lt;')
		.replace(/>/g, '&gt;')
		.replace(/"/g, '&quot;')
		.replace(/'/g, '&#039;');
}
