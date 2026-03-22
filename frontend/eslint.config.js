import tseslint from 'typescript-eslint';
import svelte from 'eslint-plugin-svelte';
import svelteParser from 'svelte-eslint-parser';

export default tseslint.config(
	// TypeScript files
	{
		files: ['**/*.ts'],
		extends: [tseslint.configs.recommended],
		rules: {
			'@typescript-eslint/no-explicit-any': 'error',
			'@typescript-eslint/no-non-null-assertion': 'error',
			'@typescript-eslint/no-unused-vars': [
				'error',
				{ argsIgnorePattern: '^_', varsIgnorePattern: '^_' }
			]
		}
	},
	// Svelte files
	{
		files: ['**/*.svelte', '**/*.svelte.ts'],
		extends: [tseslint.configs.recommended],
		plugins: {
			svelte
		},
		languageOptions: {
			parser: svelteParser,
			parserOptions: {
				parser: tseslint.parser
			}
		},
		rules: {
			'@typescript-eslint/no-explicit-any': 'error',
			'@typescript-eslint/no-non-null-assertion': 'error',
			'@typescript-eslint/no-unused-vars': [
				'error',
				{ argsIgnorePattern: '^_', varsIgnorePattern: '^_' }
			],
			// Svelte 5 $props() destructuring requires `let`, not `const`
			'prefer-const': 'off',
			// Svelte reactive expressions can look like unused expressions
			'@typescript-eslint/no-unused-expressions': 'off'
		}
	},
	// Ignore generated types and build output
	{
		ignores: [
			'src/lib/types/generated/**',
			'.svelte-kit/**',
			'build/**',
			'node_modules/**'
		]
	}
);
