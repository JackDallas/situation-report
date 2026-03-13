# Contributing to Situation Report

## Prerequisites

- **Rust** (stable, 2024 edition) — [rustup.rs](https://rustup.rs/)
- **Node.js 22+** and **pnpm** — [pnpm.io](https://pnpm.io/)
- **Docker** and **Docker Compose** — for PostgreSQL + Ollama
- **PostgreSQL 17** with TimescaleDB, PostGIS, and pgvector extensions (provided via Docker)

## Development Setup

```bash
# 1. Start the database and Ollama
docker compose up -d postgres ollama ollama-pull

# 2. Set up environment
cp .env.example .env
# Edit .env — at minimum set DB_PASSWORD

# 3. Run the backend
export DATABASE_URL="postgres://sitrep:YOUR_PASSWORD@localhost/situationreport"
cd backend && cargo run --bin sr-server

# 4. Run the frontend (separate terminal)
cd frontend && pnpm install && pnpm dev
```

The backend runs on `:3001` and the frontend dev server on `:5173`.

## Project Structure

```
backend/
├── crates/
│   ├── types/       # Shared enums (EventType, SourceType, Severity)
│   ├── config/      # Runtime configuration (SweepConfig, thresholds)
│   ├── db/          # Database queries and migrations
│   ├── sources/     # Data source implementations (27+)
│   ├── pipeline/    # Correlation rules, clustering, situation graph
│   ├── intel/       # AI narrative generation (Ollama + Claude)
│   ├── embeddings/  # BGE-M3 vector embeddings via ONNX Runtime
│   ├── server/      # Axum HTTP server and SSE endpoints
│   └── telegram-auth/ # Telegram MTProto session management
├── migrations/      # SQL migrations (run automatically on startup)
└── Cargo.toml       # Workspace manifest

frontend/
├── src/
│   ├── lib/
│   │   ├── components/  # Svelte 5 components (panels, shared, layout)
│   │   ├── services/    # API client, SSE, event display utilities
│   │   ├── stores/      # Svelte 5 runes-based reactive stores
│   │   └── types/       # TypeScript types (includes auto-generated from Rust)
│   └── routes/          # SvelteKit pages
└── static/              # GeoJSON reference data, map assets
```

## Adding a New Data Source

1. Create `backend/crates/sources/src/your_source.rs`
2. Implement the `DataSource` trait:

```rust
use crate::{DataSource, SourceContext, InsertableEvent};
use std::time::Duration;

pub struct YourSource;

impl DataSource for YourSource {
    fn id(&self) -> &str { "your_source" }
    fn name(&self) -> &str { "Your Source" }
    fn default_interval(&self) -> Duration { Duration::from_secs(300) }

    async fn poll(&self, ctx: &SourceContext) -> anyhow::Result<Vec<InsertableEvent>> {
        // Fetch data and convert to InsertableEvent structs
        todo!()
    }
}
```

3. Add the module to `backend/crates/sources/src/lib.rs`
4. Register it in `backend/crates/server/src/main.rs`
5. Add a variant to `SourceType` in `backend/crates/types/src/source_type.rs`
6. Run `cargo check` to verify

For streaming sources (WebSocket, SSE), override `is_streaming()` to return `true` and implement `start_stream()` instead of `poll()`.

## Adding a Correlation Rule

1. Create `backend/crates/pipeline/src/rules/your_rule.rs`
2. Implement the rule function that takes a time window of events and returns correlated groups
3. Register it in `backend/crates/pipeline/src/rules/mod.rs`
4. Add tests in `backend/crates/pipeline/tests/rules_tests.rs`

## Code Style

- **Rust**: Follow standard `rustfmt` conventions. Use `cargo clippy` before submitting.
- **TypeScript/Svelte**: The project uses Svelte 5 with runes (`$state`, `$derived`, `$effect`). Use Tailwind CSS for styling.
- **Commits**: Use concise, descriptive commit messages.

## Pull Requests

1. Fork the repository and create a feature branch
2. Make your changes with tests where applicable
3. Ensure `cargo check` and `cargo clippy` pass
4. Run `pnpm check` in the frontend directory
5. Submit a PR with a clear description of what changed and why
