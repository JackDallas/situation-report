import type { SituationEvent, Incident, SituationPhase, SupplementaryData } from './events';

export type SituationCategory = 'conflict' | 'cyber' | 'environmental' | 'intelligence' | 'incident';

/** Maps event_type → situation category. Types not listed here are excluded (noise). */
export const EVENT_TYPE_CATEGORY: Record<string, SituationCategory> = {
	conflict_event: 'conflict',
	thermal_anomaly: 'conflict',
	geo_event: 'conflict',
	gps_interference: 'conflict',
	internet_outage: 'cyber',
	censorship_event: 'cyber',
	bgp_leak: 'cyber',
	threat_intel: 'cyber',
	seismic_event: 'environmental',
	nuclear_event: 'environmental',
	fishing_event: 'environmental',
	news_article: 'intelligence',
	geo_news: 'intelligence',
	telegram_message: 'intelligence',
	notam_event: 'intelligence',
	economic_event: 'intelligence',
};

export const REGION_LABELS: Record<string, string> = {
	'eastern-europe': 'Eastern Europe',
	'middle-east': 'Middle East',
	'north-africa': 'North Africa',
	'sub-saharan-africa': 'Sub-Saharan Africa',
	'south-asia': 'South Asia',
	'east-asia': 'East Asia',
	'southeast-asia': 'Southeast Asia',
	'central-asia': 'Central Asia',
	'western-europe': 'Western Europe',
	'north-america': 'North America',
	'south-america': 'South America',
	'central-america': 'Central America',
	'caribbean': 'Caribbean',
	'oceania': 'Oceania',
	'arctic': 'Arctic',
	'global': 'Global',
};

export const CATEGORY_COLORS: Record<SituationCategory, { bg: string; text: string; badge: string }> = {
	conflict: { bg: 'bg-alert/10', text: 'text-alert', badge: 'bg-alert/20 text-alert' },
	cyber: { bg: 'bg-purple-500/10', text: 'text-purple-400', badge: 'bg-purple-500/20 text-purple-400' },
	environmental: { bg: 'bg-emerald-500/10', text: 'text-emerald-400', badge: 'bg-emerald-500/20 text-emerald-400' },
	intelligence: { bg: 'bg-blue-500/10', text: 'text-blue-400', badge: 'bg-blue-500/20 text-blue-400' },
	incident: { bg: 'bg-red-500/10', text: 'text-red-400', badge: 'bg-red-500/20 text-red-400' },
};

export interface Situation {
	id: string;
	title: string;
	category: SituationCategory;
	region: string;
	severity: string;
	lastUpdated: string;
	firstSeen: string;
	sourceCount: number;
	sources: string[];
	eventCount: number;
	events: SituationEvent[];
	incident: Incident | null;
	latitude: number | null;
	longitude: number | null;
	/** Parent situation ID (this is a sub-situation) */
	parentId: string | null;
	/** Child situation IDs */
	childIds: string[];
	/** Related situation IDs (cross-references) */
	relatedIds: string[];
	/** AI-generated display title (clearer than auto-generated) */
	displayTitle: string | null;
	/** Top entities from backend clustering */
	entities?: string[];
	/** Top topics from backend clustering */
	topics?: string[];
	/** Supplementary web context from Exa search (backend clusters only) */
	supplementary?: SupplementaryData | null;
	/** Lifecycle phase (backend clusters only) */
	phase?: SituationPhase | null;
	/** Timestamp of last phase change */
	phaseChangedAt?: string | null;
	/** Peak event rate */
	peakEventRate?: number | null;
	/** Certainty score (0.0–1.0) based on source diversity, event count, entities, enrichment */
	certainty?: number;
}
