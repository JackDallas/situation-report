# Situation Report

Real-time OSINT monitoring and correlation platform.

Situation Report aggregates 27+ open-source intelligence feeds, correlates events using spatial-temporal analysis, clusters them into evolving situations, and generates AI-powered narrative analysis.

## Features

- **Real-time map** with live aircraft/vessel tracking, thermal hotspots, NOTAM airspace areas, and event markers
- **27+ data sources** spanning conflict, aviation, maritime, satellite, seismic, cyber, nuclear, and news domains
- **9 correlation rules** that detect patterns like coordinated infrastructure attacks, military strike signatures, and maritime enforcement actions
- **Situation clustering** with EWMA centroids, coherence splitting, and severity propagation
- **AI narratives** generated via Ollama (local) or Claude API for each situation cluster
- **Vector embeddings** (BGE-M3) for semantic event similarity and deduplication
- **SSE streaming** for real-time dashboard updates

## Architecture

```
  Sources (27+)         Pipeline              Frontend
 ┌─────────────┐    ┌──────────────┐     ┌──────────────┐
 │ ADS-B       │    │ Correlation  │     │ SvelteKit    │
 │ AIS         │───>│ Rules (9)    │────>│ MapLibre GL  │
 │ FIRMS       │    │ Clustering   │     │ SSE Stream   │
 │ GDELT       │    │ Embeddings   │     │ Tailwind CSS │
 │ Shodan      │    │ AI Narrative │     └──────────────┘
 │ Telegram    │    └──────┬───────┘
 │ ...         │           │
 └─────────────┘    ┌──────┴───────┐
                    │ TimescaleDB  │
                    │ + PostGIS    │
                    │ + pgvector   │
                    └──────────────┘
```

## Tech Stack

| Layer     | Technology |
|-----------|-----------|
| Backend   | Rust (async, Axum) |
| Frontend  | SvelteKit 5, MapLibre GL, Tailwind CSS |
| Database  | TimescaleDB + PostGIS + pgvector |
| Embeddings| BGE-M3 via ONNX Runtime (GPU-accelerated) |
| LLM       | Ollama (local, zero-cost) or Claude API |
| Container | Docker with NVIDIA CUDA runtime |

## Quick Start

```bash
# 1. Clone and configure
cp .env.example .env
# Edit .env with your API keys (most sources work without keys)

# 2. Start everything
docker compose up -d

# 3. Open the dashboard
open http://localhost:3001
```

The app will start ingesting data from free sources immediately. Add API keys for premium sources as needed.

## Data Sources

| Source | Type | Description | API Key? |
|--------|------|-------------|----------|
| GDELT | Conflict/News | Global event database | No |
| GeoConfirmed | Conflict | Geolocated conflict events | No |
| OpenSky | Aviation | Live aircraft positions (ADS-B) | Optional |
| AirplanesLive | Aviation | ADS-B aggregator | No |
| NOTAM | Aviation | Notices to Air Missions (FAA) | Yes |
| AIS Stream | Maritime | Live vessel positions | Yes |
| Global Fishing Watch | Maritime | Vessel monitoring | Yes |
| UKMTO/ASAM | Maritime | Maritime security incidents | No |
| FIRMS | Satellite | NASA thermal hotspots | Yes |
| USGS | Seismic | Earthquake data | No |
| GDACS | Disaster | Global disaster alerts | No |
| ReliefWeb | Disaster | UN humanitarian reports | No |
| Copernicus EMS | Disaster | EU emergency activations | No |
| Shodan | Cyber | Internet-facing device intelligence | Yes |
| Cloudflare Radar | Cyber | Internet traffic anomalies | Yes |
| IODA | Cyber | Internet outage detection | No |
| BGP Stream | Cyber | BGP routing anomalies | No |
| AlienVault OTX | Cyber | Threat intelligence pulses | Yes |
| CertStream | Cyber | Certificate transparency logs | No |
| OONI | Cyber | Internet censorship detection | No |
| Nuclear | Nuclear | Radiation monitoring network | No |
| GPSJam | GPS | GPS interference detection | No |
| Telegram | OSINT | Channel monitoring (MTProto) | Yes |
| GDELT Doc | News | Document-level news analysis | No |
| GDELT Geo | News | Geolocated news events | No |
| RSS News | News | Curated RSS feed aggregation | No |

## Correlation Rules

| Rule | Pattern |
|------|---------|
| Confirmed Strike | Multi-source strike confirmation (FIRMS + conflict reports) |
| Military Strike | Military aviation near thermal anomalies |
| Conflict Thermal | Conflict events correlating with satellite hotspots |
| OSINT Strike | Telegram + news + thermal pattern matching |
| APT Staging | Cyber indicators near conflict zones |
| Coordinated Shutdown | Simultaneous internet/infrastructure outages |
| Infrastructure Attack | Physical + cyber indicators on critical infrastructure |
| GPS Military | GPS jamming near military installations |
| Maritime Enforcement | Naval activity patterns near maritime incidents |

## Environment Variables

See [`.env.example`](.env.example) for the full list. Key variables:

| Variable | Required | Description |
|----------|----------|-------------|
| `DB_PASSWORD` | Yes | PostgreSQL password |
| `ANTHROPIC_API_KEY` | No | Claude API for AI narratives (falls back to Ollama) |
| `OLLAMA_MODEL` | No | Local LLM model (default: `qwen3.5:9b`) |
| `SHODAN_API_KEY` | No | Shodan intelligence feeds |
| `FIRMS_MAP_KEY` | No | NASA FIRMS thermal data |
| `AISSTREAM_API_KEY` | No | Maritime vessel tracking |

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, adding new sources, and contribution guidelines.

## License

[MIT](LICENSE)
