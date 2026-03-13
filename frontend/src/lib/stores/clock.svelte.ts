/** Reactive clock that ticks every 10 seconds. Use `clockStore.now` in $derived to get live relative times. */
class ClockStore {
	now = $state(Date.now());
	private interval: ReturnType<typeof setInterval> | null = null;

	start() {
		if (this.interval) return;
		this.interval = setInterval(() => {
			this.now = Date.now();
		}, 5_000);
	}

	stop() {
		if (this.interval) {
			clearInterval(this.interval);
			this.interval = null;
		}
	}
}

export const clockStore = new ClockStore();

// Auto-start on import
clockStore.start();
