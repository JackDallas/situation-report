/** Military/national affiliation color map for map markers and position interpolation */
export const AFFILIATION_COLORS: Record<string, string> = {
	'US': '#3b82f6',    // blue
	'RU': '#ef4444',    // red
	'CN': '#f97316',    // orange
	'IL': '#22d3ee',    // cyan
	'IR': '#10b981',    // green
	'UA': '#eab308',    // yellow
	'GB': '#6366f1',    // indigo
	'FR': '#8b5cf6',    // violet
	'DE': '#ec4899',    // pink
	'NATO': '#818cf8',  // light indigo
};

export const DEFAULT_MIL_COLOR = '#f472b6';   // pink (unknown military)
export const CIVILIAN_COLOR = '#64748b';       // slate
export const VESSEL_COLOR = '#06b6d4';         // cyan

/** Entity type badge colors for intelligence reports */
export const ENTITY_TYPE_ICONS: Record<string, string> = {
	person: 'bg-purple-500/20 text-purple-400',
	organization: 'bg-blue-500/20 text-blue-400',
	location: 'bg-emerald-500/20 text-emerald-400',
	weapon_system: 'bg-red-500/20 text-red-400',
	military_unit: 'bg-orange-500/20 text-orange-400',
};

/** Severity ranking for sorting/comparison */
export const SEVERITY_RANK: Record<string, number> = { critical: 4, high: 3, medium: 2, low: 1 };

export function severityRank(s: string): number {
	return SEVERITY_RANK[s] ?? 0;
}
