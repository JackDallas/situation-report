import type { SituationEvent, Incident, Summary, AnalysisReport } from '$lib/types/events';

export type ConnectionStatus = 'connected' | 'reconnecting' | 'disconnected';

class EventStore {
	events = $state<SituationEvent[]>([]);
	incidents = $state<Incident[]>([]);
	summaries = $state<Map<string, Summary>>(new Map());
	latestAnalysis = $state<AnalysisReport | null>(null);
	connectionStatus = $state<ConnectionStatus>('disconnected');
	selectedEvent = $state<SituationEvent | null>(null);
	selectedIncident = $state<Incident | null>(null);

	private maxEvents = 500;
	private maxIncidents = 100;

	addEvent(event: SituationEvent) {
		const next = this.events.slice();
		next.unshift(event);
		if (next.length > this.maxEvents) {
			next.length = this.maxEvents;
		}
		this.events = next;
	}

	addEvents(events: SituationEvent[]) {
		const next = [...events, ...this.events];
		if (next.length > this.maxEvents) {
			next.length = this.maxEvents;
		}
		this.events = next;
	}

	addIncident(incident: Incident) {
		const next = this.incidents.slice();
		next.unshift(incident);
		if (next.length > this.maxIncidents) {
			next.length = this.maxIncidents;
		}
		this.incidents = next;
	}

	updateSummary(summary: Summary) {
		const next = new Map(this.summaries);
		next.set(summary.event_type, summary);
		this.summaries = next;
	}

	updateAnalysis(report: AnalysisReport) {
		this.latestAnalysis = report;
	}

	eventsBySource = $derived.by(() => {
		const grouped: Record<string, SituationEvent[]> = {};
		for (const event of this.events) {
			const key = event.source_type;
			if (!grouped[key]) grouped[key] = [];
			grouped[key].push(event);
		}
		return grouped;
	});

	eventsByType = $derived.by(() => {
		const grouped: Record<string, SituationEvent[]> = {};
		for (const event of this.events) {
			const key = event.event_type;
			if (!grouped[key]) grouped[key] = [];
			grouped[key].push(event);
		}
		return grouped;
	});

	get eventCount() {
		return this.events.length;
	}

	get incidentCount() {
		return this.incidents.length;
	}

	clear() {
		this.events = [];
		this.incidents = [];
		this.summaries = new Map();
	}
}

export const eventStore = new EventStore();
