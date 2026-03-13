export type Theme = 'dark' | 'light';

const STORAGE_KEY = 'sr-theme';

class ThemeStore {
	current = $state<Theme>('dark');

	constructor() {
		if (typeof window !== 'undefined') {
			const saved = localStorage.getItem(STORAGE_KEY);
			if (saved === 'light' || saved === 'dark') {
				this.current = saved;
			}
			this.apply();
		}
	}

	toggle() {
		this.current = this.current === 'dark' ? 'light' : 'dark';
		this.apply();
	}

	private apply() {
		if (typeof document !== 'undefined') {
			document.documentElement.setAttribute('data-theme', this.current);
			localStorage.setItem(STORAGE_KEY, this.current);
		}
	}
}

export const themeStore = new ThemeStore();

export function toggleTheme() {
	themeStore.toggle();
}
