/**
 * Markdown-to-HTML renderer using `marked` with DOMPurify sanitization.
 * Replaces the hand-rolled regex renderer with a proper parser + sanitizer.
 */

import { Marked } from 'marked';
import DOMPurify from 'dompurify';

const marked = new Marked({
	breaks: true,
	gfm: true
});

// Configure DOMPurify: allow safe formatting tags only
const PURIFY_CONFIG = {
	ALLOWED_TAGS: [
		'p', 'br', 'strong', 'b', 'em', 'i', 'ul', 'ol', 'li',
		'hr', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6',
		'blockquote', 'code', 'pre', 'a', 'table', 'thead',
		'tbody', 'tr', 'th', 'td', 'del', 'sup', 'sub'
	],
	ALLOWED_ATTR: ['href', 'title', 'class'],
	ALLOW_DATA_ATTR: false,
	RETURN_TRUSTED_TYPE: false
};

/**
 * Convert markdown text to sanitized HTML.
 * Uses `marked` for parsing and DOMPurify for XSS protection.
 */
export function renderMarkdown(input: string): string {
	if (!input) return '';

	const raw = marked.parse(input) as string;
	return DOMPurify.sanitize(raw, PURIFY_CONFIG) as string;
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
