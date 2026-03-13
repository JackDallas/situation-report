// AIS ship type codes (ITU-R M.1371)
export const SHIP_TYPES: Record<number, string> = {
	0: 'Not available',
	20: 'Wing in ground',
	30: 'Fishing',
	31: 'Towing',
	32: 'Towing (large)',
	33: 'Dredging/underwater ops',
	34: 'Diving operations',
	35: 'Military operations',
	36: 'Sailing',
	37: 'Pleasure craft',
	40: 'High speed craft',
	50: 'Pilot vessel',
	51: 'Search and rescue',
	52: 'Tug',
	53: 'Port tender',
	54: 'Anti-pollution',
	55: 'Law enforcement',
	56: 'Spare (local)',
	57: 'Spare (local)',
	58: 'Medical transport',
	59: 'RR Resolution No.18',
	60: 'Passenger',
	69: 'Passenger (no additional info)',
	70: 'Cargo',
	79: 'Cargo (no additional info)',
	80: 'Tanker',
	89: 'Tanker (no additional info)',
	90: 'Other',
	99: 'Other (no additional info)'
};

export function getShipTypeName(code: number | string | undefined): string {
	if (code == null) return 'Unknown';
	const n = typeof code === 'string' ? parseInt(code) : code;
	if (isNaN(n)) return 'Unknown';
	// Exact match first
	if (SHIP_TYPES[n]) return SHIP_TYPES[n];
	// Range match (61-68 = Passenger variants, 71-78 = Cargo variants, etc.)
	if (n >= 60 && n <= 69) return 'Passenger';
	if (n >= 70 && n <= 79) return 'Cargo';
	if (n >= 80 && n <= 89) return 'Tanker';
	if (n >= 40 && n <= 49) return 'High speed craft';
	if (n >= 20 && n <= 29) return 'Wing in ground';
	return `Type ${n}`;
}

// AIS navigation status codes
export const NAV_STATUS: Record<number, string> = {
	0: 'Under way using engine',
	1: 'At anchor',
	2: 'Not under command',
	3: 'Restricted manoeuvrability',
	4: 'Constrained by draught',
	5: 'Moored',
	6: 'Aground',
	7: 'Engaged in fishing',
	8: 'Under way sailing',
	9: 'Reserved (HSC)',
	10: 'Reserved (WIG)',
	11: 'Power-driven vessel towing astern',
	12: 'Power-driven vessel pushing ahead',
	14: 'AIS-SART active',
	15: 'Not defined'
};

export function getNavStatusName(code: number | string | undefined): string {
	if (code == null) return 'Unknown';
	const n = typeof code === 'string' ? parseInt(code) : code;
	if (isNaN(n)) return String(code);
	return NAV_STATUS[n] ?? `Status ${n}`;
}
