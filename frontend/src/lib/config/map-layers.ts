export interface MapLayerConfig {
	id: string;
	name: string;
	sourceType: string;
	color: string;
	enabled: boolean;
}

export const mapLayers: MapLayerConfig[] = [
	{
		id: 'conflicts',
		name: 'Conflict Events',
		sourceType: 'conflict_event',
		color: '#ef4444',
		enabled: true,
	},
	{
		id: 'thermal',
		name: 'Thermal Anomalies',
		sourceType: 'thermal_anomaly',
		color: '#f97316',
		enabled: true,
	},
	{
		id: 'flights',
		name: 'Military Flights',
		sourceType: 'flight_position',
		color: '#3b82f6',
		enabled: false,
	},
	{
		id: 'vessels',
		name: 'Vessel Positions',
		sourceType: 'vessel_position',
		color: '#06b6d4',
		enabled: false,
	},
	{
		id: 'outages',
		name: 'Internet Outages',
		sourceType: 'internet_outage',
		color: '#a855f7',
		enabled: true,
	},
	{
		id: 'shodan',
		name: 'Shodan ICS',
		sourceType: 'shodan_banner',
		color: '#0ea5e9',
		enabled: false,
	},
];
