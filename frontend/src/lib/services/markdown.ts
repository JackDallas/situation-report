/**
 * Lightweight markdown-to-HTML renderer with sanitization.
 * Handles: **bold**, *italic*, ---, ***, - list items, paragraph breaks.
 * Strips all raw HTML tags from input for safety.
 */

/** Strip all HTML tags from input to prevent XSS */
function sanitize(text: string): string {
	return text.replace(/<[^>]*>/g, '');
}

/** Render inline markdown: **bold** and *italic* */
function renderInline(line: string): string {
	// Bold: **text**
	line = line.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
	// Italic: *text* (but not inside strong tags)
	line = line.replace(/(?<!\w)\*([^*]+?)\*(?!\w)/g, '<em>$1</em>');
	return line;
}

/**
 * Convert simple markdown text to sanitized HTML.
 *
 * Supported syntax:
 * - `**bold**` -> `<strong>`
 * - `*italic*` -> `<em>`
 * - `---` or `***` (alone on a line) -> `<hr>`
 * - Lines starting with `- ` -> `<ul><li>` items
 * - Blank lines (`\n\n`) -> paragraph breaks
 * - All HTML tags stripped from input
 */
export function renderMarkdown(input: string): string {
	if (!input) return '';

	// Sanitize first — strip any raw HTML
	const clean = sanitize(input);

	// Split into paragraphs on double newlines
	const blocks = clean.split(/\n\n+/);
	const htmlParts: string[] = [];

	for (const block of blocks) {
		const trimmed = block.trim();
		if (!trimmed) continue;

		// Check if entire block is a horizontal rule
		if (/^(-{3,}|\*{3,})$/.test(trimmed)) {
			htmlParts.push('<hr class="my-2 border-border-default" />');
			continue;
		}

		// Split block into individual lines
		const lines = trimmed.split('\n');

		// Check if this block is a list (all non-empty lines start with "- ")
		const nonEmpty = lines.filter((l) => l.trim().length > 0);
		const isList = nonEmpty.length > 0 && nonEmpty.every((l) => l.trim().startsWith('- '));

		if (isList) {
			const items = nonEmpty.map((l) => {
				const content = l.trim().replace(/^- /, '');
				return `<li>${renderInline(content)}</li>`;
			});
			htmlParts.push(`<ul class="list-disc pl-4 space-y-0.5">${items.join('')}</ul>`);
			continue;
		}

		// Mixed content: some lines may be list items, some not.
		// Also handle lines that are horizontal rules within a block.
		let currentParagraph: string[] = [];
		let currentList: string[] = [];

		function flushParagraph() {
			if (currentParagraph.length > 0) {
				const text = currentParagraph.join(' ');
				htmlParts.push(`<p>${renderInline(text)}</p>`);
				currentParagraph = [];
			}
		}

		function flushList() {
			if (currentList.length > 0) {
				htmlParts.push(
					`<ul class="list-disc pl-4 space-y-0.5">${currentList.map((i) => `<li>${renderInline(i)}</li>`).join('')}</ul>`
				);
				currentList = [];
			}
		}

		for (const line of lines) {
			const t = line.trim();
			if (!t) continue;

			if (/^(-{3,}|\*{3,})$/.test(t)) {
				flushParagraph();
				flushList();
				htmlParts.push('<hr class="my-2 border-border-default" />');
			} else if (t.startsWith('- ')) {
				flushParagraph();
				currentList.push(t.replace(/^- /, ''));
			} else {
				flushList();
				currentParagraph.push(t);
			}
		}

		flushParagraph();
		flushList();
	}

	return htmlParts.join('');
}

/**
 * Split markdown into paragraphs (for truncation / "Read more" logic).
 * Returns an array of paragraph strings (raw markdown, not rendered).
 */
export function splitMarkdownParagraphs(input: string): string[] {
	if (!input) return [];
	return input
		.split(/\n\n+/)
		.map((p) => p.trim())
		.filter((p) => p.length > 0);
}
