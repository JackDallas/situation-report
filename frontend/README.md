# Situation Report — Frontend

SvelteKit 5 dashboard for the Situation Report OSINT platform.

## Setup

```bash
pnpm install
pnpm dev
```

Dev server runs on `http://localhost:5173` and proxies API requests to the backend on `:3001`.

## Stack

- **SvelteKit 5** with runes (`$state`, `$derived`, `$effect`)
- **MapLibre GL** for the interactive map
- **Tailwind CSS 4** for styling
- **D3-force** for graph visualizations
- **SSE** for real-time event streaming

## Structure

```
src/lib/
├── components/
│   ├── layout/    # Main layout shell, command palette
│   ├── panels/    # Map, alerts, news, situations, intel brief
│   └── shared/    # Reusable UI (status bar, badges, timeline)
├── services/      # API client, SSE connection, event display
├── stores/        # Reactive stores (map state, sources, UI)
└── types/         # TypeScript types (some auto-generated from Rust via ts-rs)
```

## Generated Types

Some TypeScript types in `src/lib/types/generated/` are auto-generated from Rust structs using [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit these files directly.
