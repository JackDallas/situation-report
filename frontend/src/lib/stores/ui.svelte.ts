import type { PositionEntry } from '$lib/stores/map.svelte';

export type RightPanel = 'sitreps' | 'news' | 'event-detail' | 'situation-detail' | 'position-detail' | 'domain' | 'intel-brief' | null;
export type DomainTab = 'kinetic' | 'cyber' | 'track' | 'intel' | 'flow';

class UIStore {
	rightPanel = $state<RightPanel>('intel-brief');
	rightCollapsed = $state(false);
	domainTab = $state<DomainTab>('kinetic');

	/** Currently selected position entity for the detail pane. */
	selectedPosition = $state<PositionEntry | null>(null);

	openPanel(panel: RightPanel) {
		this.rightPanel = panel;
		this.rightCollapsed = false;
	}

	/** Return to the default right panel (intel brief). */
	openDefault() {
		this.rightPanel = 'intel-brief';
		this.rightCollapsed = false;
	}

	/** Open the position detail pane for a given position entry. */
	openPositionDetail(position: PositionEntry) {
		this.selectedPosition = position;
		this.rightPanel = 'position-detail';
		this.rightCollapsed = false;
	}

	toggleRight() {
		this.rightCollapsed = !this.rightCollapsed;
	}
}

export const uiStore = new UIStore();
