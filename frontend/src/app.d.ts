// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}

	// Bridge functions for MapPanel popup -> drawer communication
	interface Window {
		__srDetailProps?: Record<string, unknown>[];
		__srOpenDetail?: (idx: number) => void;
		__srOpenSituation?: (situationId: string) => void;
	}
}

export {};
