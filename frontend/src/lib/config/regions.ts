export interface RegionConfig {
	id: string;
	name: string;
	bbox: [number, number, number, number]; // [west, south, east, north]
	center: [number, number]; // [lng, lat]
	zoom: number;
}

export const regions: RegionConfig[] = [
	{
		id: 'middle_east',
		name: 'Middle East',
		bbox: [25.0, 12.0, 63.0, 42.0],
		center: [44.0, 27.0],
		zoom: 4,
	},
	{
		id: 'ukraine',
		name: 'Ukraine / Russia',
		bbox: [22.0, 44.0, 40.0, 53.0],
		center: [31.0, 48.5],
		zoom: 5,
	},
	{
		id: 'sudan',
		name: 'Sudan',
		bbox: [21.8, 3.5, 38.6, 23.1],
		center: [30.2, 13.3],
		zoom: 5,
	},
	{
		id: 'taiwan_strait',
		name: 'Taiwan Strait',
		bbox: [117.0, 21.0, 123.0, 27.0],
		center: [120.0, 24.0],
		zoom: 6,
	},
	{
		id: 'myanmar',
		name: 'Myanmar',
		bbox: [92.0, 9.5, 101.2, 28.5],
		center: [96.6, 19.0],
		zoom: 5,
	},
	{
		id: 'western_europe',
		name: 'Western Europe',
		bbox: [-10.0, 36.0, 15.0, 60.0],
		center: [2.0, 48.0],
		zoom: 4,
	},
	{
		id: 'eastern_europe',
		name: 'Eastern Europe',
		bbox: [15.0, 40.0, 45.0, 57.0],
		center: [31.0, 48.5],
		zoom: 4,
	},
	{
		id: 'africa',
		name: 'Africa',
		bbox: [-18.0, -35.0, 52.0, 37.0],
		center: [25.0, 8.0],
		zoom: 3,
	},
	{
		id: 'east_asia',
		name: 'East Asia',
		bbox: [100.0, 20.0, 150.0, 50.0],
		center: [120.0, 35.0],
		zoom: 4,
	},
	{
		id: 'south_asia',
		name: 'South Asia',
		bbox: [60.0, 5.0, 98.0, 40.0],
		center: [78.0, 25.0],
		zoom: 4,
	},
	{
		id: 'central_asia',
		name: 'Central Asia',
		bbox: [50.0, 35.0, 80.0, 50.0],
		center: [65.0, 42.0],
		zoom: 4,
	},
	{
		id: 'north_america',
		name: 'North America',
		bbox: [-170.0, 15.0, -50.0, 72.0],
		center: [-100.0, 40.0],
		zoom: 3,
	},
	{
		id: 'south_america',
		name: 'South America',
		bbox: [-82.0, -56.0, -34.0, 13.0],
		center: [-55.0, -15.0],
		zoom: 3,
	},
	{
		id: 'oceania',
		name: 'Oceania',
		bbox: [110.0, -50.0, 180.0, 0.0],
		center: [135.0, -25.0],
		zoom: 3,
	},
	{
		id: 'southeast_asia',
		name: 'Southeast Asia',
		bbox: [90.0, -10.0, 140.0, 25.0],
		center: [105.0, 15.0],
		zoom: 4,
	},
];

/**
 * Look up the center [lng, lat] for a region name.
 * Performs fuzzy matching: lowercases the input and normalizes hyphens,
 * underscores, and spaces so that "eastern-europe", "eastern_europe",
 * and "Eastern Europe" all match.
 *
 * Returns null if no region matches.
 */
export function getRegionCenter(regionName: string): [number, number] | null {
	const normalized = regionName.toLowerCase().replace(/[-\s]/g, '_');

	// Direct match on id
	const direct = regions.find((r) => r.id === normalized);
	if (direct) return direct.center;

	// Also try matching the name (normalized)
	const byName = regions.find(
		(r) => r.name.toLowerCase().replace(/[-\s]/g, '_') === normalized,
	);
	if (byName) return byName.center;

	// Partial / alias matching for common backend region codes
	const aliases: Record<string, string> = {
		'middle_east': 'middle_east',
		'eastern_europe': 'eastern_europe',
		'western_europe': 'western_europe',
		'east_asia': 'east_asia',
		'south_asia': 'south_asia',
		'central_asia': 'central_asia',
		'north_america': 'north_america',
		'south_america': 'south_america',
		'southeast_asia': 'southeast_asia',
		'sub_saharan_africa': 'africa',
		'north_africa': 'africa',
		'central_america': 'south_america',
		'caribbean': 'south_america',
		'arctic': 'eastern_europe',
		'global': 'middle_east',
	};

	const aliasTarget = aliases[normalized];
	if (aliasTarget) {
		const found = regions.find((r) => r.id === aliasTarget);
		if (found) return found.center;
	}

	return null;
}
