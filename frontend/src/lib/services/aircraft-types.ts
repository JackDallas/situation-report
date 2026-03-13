// Common ICAO type designators for military and notable aircraft
export const AIRCRAFT_TYPES: Record<string, { name: string; role: string }> = {
	// US Military
	F16: { name: 'F-16 Fighting Falcon', role: 'Fighter' },
	F15: { name: 'F-15 Eagle', role: 'Fighter' },
	F18S: { name: 'F/A-18 Super Hornet', role: 'Fighter' },
	F35: { name: 'F-35A Lightning II', role: 'Stealth Fighter' },
	F35B: { name: 'F-35B Lightning II', role: 'STOVL Fighter' },
	F35C: { name: 'F-35C Lightning II', role: 'Carrier Fighter' },
	F22: { name: 'F-22 Raptor', role: 'Air Superiority' },
	A10: { name: 'A-10 Thunderbolt II', role: 'Ground Attack' },
	B52: { name: 'B-52 Stratofortress', role: 'Strategic Bomber' },
	B1: { name: 'B-1B Lancer', role: 'Strategic Bomber' },
	B2: { name: 'B-2 Spirit', role: 'Stealth Bomber' },
	C130: { name: 'C-130 Hercules', role: 'Transport' },
	C17: { name: 'C-17 Globemaster III', role: 'Strategic Transport' },
	C5M: { name: 'C-5M Super Galaxy', role: 'Strategic Transport' },
	KC135: { name: 'KC-135 Stratotanker', role: 'Aerial Refueling' },
	KC10: { name: 'KC-10 Extender', role: 'Aerial Refueling' },
	KC46: { name: 'KC-46 Pegasus', role: 'Aerial Refueling' },
	E3CF: { name: 'E-3 Sentry (AWACS)', role: 'Airborne Early Warning' },
	E6B: { name: 'E-6B Mercury', role: 'TACAMO' },
	E8: { name: 'E-8 JSTARS', role: 'Ground Surveillance' },
	P8: { name: 'P-8A Poseidon', role: 'Maritime Patrol' },
	P3: { name: 'P-3 Orion', role: 'Maritime Patrol' },
	RQ4: { name: 'RQ-4 Global Hawk', role: 'HALE ISR' },
	MQ9: { name: 'MQ-9 Reaper', role: 'MALE ISR/Strike' },
	MQ1: { name: 'MQ-1 Predator', role: 'ISR' },
	RC135: { name: 'RC-135', role: 'Reconnaissance' },
	U2: { name: 'U-2 Dragon Lady', role: 'High-altitude Recon' },
	UH60: { name: 'UH-60 Black Hawk', role: 'Utility Helicopter' },
	AH64: { name: 'AH-64 Apache', role: 'Attack Helicopter' },
	CH47: { name: 'CH-47 Chinook', role: 'Heavy Lift Helicopter' },
	V22: { name: 'V-22 Osprey', role: 'Tiltrotor' },
	BALL: { name: 'Aerosonde Balloon', role: 'Surveillance' },
	// UK Military
	EUFI: { name: 'Eurofighter Typhoon', role: 'Fighter' },
	TEX2: { name: 'Beechcraft T-6 Texan II', role: 'Trainer' },
	HAWK: { name: 'BAE Hawk', role: 'Trainer/Light Attack' },
	A400: { name: 'Airbus A400M Atlas', role: 'Transport' },
	C30J: { name: 'C-130J Super Hercules', role: 'Transport' },
	VOYG: { name: 'Airbus Voyager (A330 MRTT)', role: 'Tanker/Transport' },
	RIAT: { name: 'Rivet Joint (RC-135W)', role: 'SIGINT' },
	// Russia
	SU27: { name: 'Sukhoi Su-27 Flanker', role: 'Fighter' },
	SU30: { name: 'Sukhoi Su-30 Flanker-C', role: 'Multirole Fighter' },
	SU34: { name: 'Sukhoi Su-34 Fullback', role: 'Fighter-Bomber' },
	SU35: { name: 'Sukhoi Su-35 Flanker-E', role: 'Air Superiority' },
	SU57: { name: 'Sukhoi Su-57 Felon', role: 'Stealth Fighter' },
	TU95: { name: 'Tupolev Tu-95 Bear', role: 'Strategic Bomber' },
	TU160: { name: 'Tupolev Tu-160 Blackjack', role: 'Strategic Bomber' },
	TU22: { name: 'Tupolev Tu-22M Backfire', role: 'Bomber' },
	IL76: { name: 'Ilyushin Il-76 Candid', role: 'Transport' },
	AN124: { name: 'Antonov An-124 Condor', role: 'Heavy Transport' },
	A50: { name: 'Beriev A-50 Mainstay', role: 'AEW&C' },
	// Israel
	F16I: { name: 'F-16I Sufa', role: 'Fighter' },
	F15I: { name: "F-15I Ra'am", role: 'Strike Fighter' },
	// Common civilian for reference
	B738: { name: 'Boeing 737-800', role: 'Airliner' },
	B77W: { name: 'Boeing 777-300ER', role: 'Airliner' },
	A320: { name: 'Airbus A320', role: 'Airliner' },
	A388: { name: 'Airbus A380', role: 'Airliner' }
};

export function getAircraftInfo(
	typeCode: string | undefined
): { name: string; role: string } | null {
	if (!typeCode) return null;
	return AIRCRAFT_TYPES[typeCode.toUpperCase()] ?? null;
}

// ICAO category codes
export const AIRCRAFT_CATEGORIES: Record<string, string> = {
	A0: 'No category info',
	A1: 'Light (< 15,500 lbs)',
	A2: 'Small (15,500-75,000 lbs)',
	A3: 'Large (75,000-300,000 lbs)',
	A4: 'High vortex large',
	A5: 'Heavy (> 300,000 lbs)',
	A6: 'High performance',
	A7: 'Rotorcraft',
	B0: 'No category info',
	B1: 'Glider / sailplane',
	B2: 'Lighter-than-air',
	B3: 'Parachutist / skydiver',
	B4: 'Ultralight / hang-glider',
	B6: 'UAV / drone',
	B7: 'Space vehicle',
	C0: 'No category info',
	C1: 'Surface vehicle - emergency',
	C2: 'Surface vehicle - service',
	C3: 'Fixed ground obstruction'
};

export function getAircraftCategory(code: string | undefined): string {
	if (!code) return 'Unknown';
	return AIRCRAFT_CATEGORIES[code] ?? `Category ${code}`;
}

// Emergency squawk codes
export function decodeSquawk(
	squawk: string | undefined
): { text: string; alert: boolean } | null {
	if (!squawk) return null;
	switch (squawk) {
		case '7500':
			return { text: 'HIJACK', alert: true };
		case '7600':
			return { text: 'RADIO FAILURE', alert: true };
		case '7700':
			return { text: 'EMERGENCY', alert: true };
		default:
			return null;
	}
}
