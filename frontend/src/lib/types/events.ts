// Re-export generated types (single source of truth from Rust backend via ts-rs)
export type {
	EventType,
	Severity,
	SourceType,
	EvidenceRole,
	SituationPhase,
	SituationEvent,
	Incident,
	EvidenceRef,
	Summary,
	SituationCluster,
	AnalysisReport,
	SuggestedMerge,
	TopicCluster,
	EntityConnection,
	PublishEvent,
	FiredAlert,
	AlertRule,
	BudgetStatus,
	SearchArticle,
	SupplementaryData
} from './generated';

// ---------- Frontend-only types (no backend equivalent) ----------

export interface GeoJSONFeatureCollection {
	type: 'FeatureCollection';
	features: GeoJSONFeature[];
}

export interface GeoJSONFeature {
	type: 'Feature';
	geometry: {
		type: 'Point';
		coordinates: [number, number];
	};
	properties: {
		source_type: string;
		source_id: string | null;
		event_type: string;
		event_time: string;
		entity_id: string | null;
		entity_name: string | null;
		severity: string | null;
		confidence: number | null;
		title: string | null;
		region_code: string | null;
		payload?: Record<string, unknown>;
		/** ICAO location code for notam_event features (promoted from payload for symbol layer) */
		notam_location?: string;
		/** NOTAM radius in nautical miles (promoted from payload for area polygon rendering) */
		notam_radius_nm?: number;
	};
}
