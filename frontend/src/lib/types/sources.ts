export interface SourceConfig {
	source_id: string;
	enabled: boolean;
	poll_interval_secs: number | null;
	api_key_encrypted: string | null;
	extra_config: Record<string, unknown>;
	updated_at: string;
}

export interface SourceHealth {
	source_id: string;
	last_success: string | null;
	last_failure: string | null;
	last_error: string | null;
	consecutive_failures: number;
	total_events_24h: number;
	status: 'healthy' | 'degraded' | 'down' | 'unknown';
}

export interface SourceInfo {
	id: string;
	name: string;
	config: SourceConfig | null;
	health: SourceHealth | null;
}
