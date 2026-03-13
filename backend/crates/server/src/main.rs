use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use axum::routing::{delete, get, post};
use axum::Router;
use sr_intel::{BudgetManager, ClaudeClient, OllamaClient};
use sr_sources::InsertableEvent;
use sr_types::Severity;
use sr_sources::registry::SourceRegistry;
use tokio::sync::broadcast;
use sr_pipeline::{spawn_pipeline, PipelineConfig, AirspaceIndex, SharedAirspaceIndex};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

mod error;
mod routes;
mod state;
mod static_files;

use state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file (looks in cwd and parent directories)
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Database
    let database_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgres://sitrep:password@localhost/situationreport".to_string()
        });
    let pool = sr_sources::db::connect(&database_url).await?;
    sr_sources::db::run_migrations(&pool).await?;
    info!("Database connected and migrations applied");

    // SSE broadcast channel (4096 event buffer)
    let (event_tx, _) = broadcast::channel::<InsertableEvent>(4096);

    // Source registry — register all data sources
    let mut registry = SourceRegistry::new();

    // Shodan (alert stream + discovery + ICS monitoring)
    registry.register(Arc::new(sr_sources::shodan::ShodanStream::new()));
    registry.register(Arc::new(sr_sources::shodan::ShodanDiscovery::new()));
    registry.register(Arc::new(sr_sources::shodan::ShodanSearch::new()));

    // Conflict data (ACLED disabled — requires paid Research tier)
    // registry.register(Arc::new(sr_sources::acled::AcledSource::new()));
    registry.register(Arc::new(sr_sources::gdelt::GdeltSource::new()));
    registry.register(Arc::new(sr_sources::geoconfirmed::GeoConfirmedSource::new()));

    // Aviation + Maritime
    registry.register(Arc::new(sr_sources::opensky::OpenSkySource::new()));
    // ADS-B flight tracking (readsb-compatible aggregators)
    registry.register(Arc::new(sr_sources::adsb::airplaneslive()));
    registry.register(Arc::new(sr_sources::adsb::adsb_lol()));
    registry.register(Arc::new(sr_sources::adsb::adsb_fi()));
    registry.register(Arc::new(sr_sources::ais::AisSource));

    // NOTAMs / Airspace
    registry.register(Arc::new(sr_sources::notam::NotamSource::new()));

    // Satellite / Thermal
    registry.register(Arc::new(sr_sources::firms::FirmsSource::new()));

    // Seismic
    registry.register(Arc::new(sr_sources::usgs::UsgsSource::new()));

    // Multi-hazard disaster alerts (earthquakes, cyclones, floods, volcanoes, wildfires, droughts)
    registry.register(Arc::new(sr_sources::gdacs::GdacsSource::new()));

    // Nuclear / Radiological
    registry.register(Arc::new(sr_sources::nuclear::NuclearSource::new()));

    // Cyber + Infrastructure
    registry.register(Arc::new(sr_sources::cloudflare::CloudflareSource::new()));
    registry.register(Arc::new(sr_sources::cloudflare::CloudflareBgpSource::new()));
    registry.register(Arc::new(sr_sources::ioda::IodaSource::new()));
    registry.register(Arc::new(sr_sources::bgp::BgpSource));
    registry.register(Arc::new(sr_sources::otx::OtxSource::new()));
    registry.register(Arc::new(sr_sources::certstream::CertstreamSource));
    registry.register(Arc::new(sr_sources::ooni::OoniSource::new()));

    // GDELT GEO 2.0
    registry.register(Arc::new(sr_sources::gdelt_geo::GdeltGeoSource::new()));

    // Maritime — Global Fishing Watch
    registry.register(Arc::new(sr_sources::gfw::GfwSource::new()));

    // GPS/Navigation interference
    registry.register(Arc::new(sr_sources::gpsjam::GpsJamSource::new()));

    // Telegram OSINT
    registry.register(Arc::new(sr_sources::telegram::TelegramSource::new()));

    // RSS News Feeds (Reuters, BBC, Al Jazeera, RFE/RL, etc.)
    registry.register(Arc::new(sr_sources::rss_news::RssNewsSource::new()));

    // Maritime Security (NGA ASAM — piracy, missile attacks, hijackings)
    registry.register(Arc::new(sr_sources::ukmto::UkmtoSource::new()));

    // ReliefWeb (UN OCHA — humanitarian reports, disaster declarations)
    registry.register(Arc::new(sr_sources::reliefweb::ReliefwebSource::new()));

    // Copernicus Emergency Management Service (emergency mapping activations)
    registry.register(Arc::new(sr_sources::copernicus::CopernicusSource::new()));

    let registry = Arc::new(registry);

    // Start all registered sources
    registry.start_all(pool.clone(), event_tx.clone());

    // Intelligence layer — Claude API client + budget manager
    let claude_client = match ClaudeClient::from_env() {
        Ok(client) => {
            info!("Claude API client initialized — intelligence enrichment enabled");
            Some(Arc::new(client))
        }
        Err(_) => {
            warn!("ANTHROPIC_API_KEY not set — intelligence enrichment disabled");
            None
        }
    };
    let budget = BudgetManager::from_db(pool.clone()).await;

    // Local LLM for enrichment (Ollama + GPU)
    let ollama_client = OllamaClient::from_env();
    if let Some(ref oc) = ollama_client {
        if oc.is_ready().await {
            info!(model = oc.model(), "Ollama connected — local GPU enrichment enabled");
        } else {
            warn!(model = oc.model(), "Ollama connected but model not yet loaded — will retry");
        }
    } else {
        info!("OLLAMA_URL not set — using Claude API for enrichment");
    }

    // Entity graph — load from DB, build in-memory resolver + graph
    let entity_resolver = {
        let entities = sr_pipeline::entity_graph::queries::load_all_entities(&pool).await.unwrap_or_default();
        let relationships = sr_pipeline::entity_graph::queries::load_all_relationships(&pool).await.unwrap_or_default();
        let mut resolver = sr_pipeline::entity_graph::EntityResolver::new();
        resolver.load(entities.clone());
        let mut graph = sr_pipeline::entity_graph::EntityGraph::new();
        graph.load(&entities, &relationships);
        info!(
            entities = resolver.len(),
            relationships = graph.edge_count(),
            "Entity graph loaded from DB"
        );
        (
            Arc::new(std::sync::RwLock::new(resolver)),
            Arc::new(std::sync::RwLock::new(graph)),
        )
    };
    let shared_entity_resolver: sr_pipeline::SharedEntityResolver = entity_resolver.0;
    let shared_entity_graph: sr_pipeline::SharedEntityGraph = entity_resolver.1;

    // Embedding model — BGE-M3 for semantic event clustering
    let embedding_model = if std::env::var("EMBEDDINGS_ENABLED")
        .unwrap_or_else(|_| "true".to_string())
        .parse::<bool>()
        .unwrap_or(true)
    {
        match sr_embeddings::EmbeddingModel::try_new() {
            Ok(m) => {
                info!("BGE-M3 loaded — semantic clustering enabled");
                Some(Arc::new(m))
            }
            Err(e) => {
                warn!("Embedding model failed to load: {e} — semantic clustering disabled");
                None
            }
        }
    } else {
        info!("Embeddings disabled via EMBEDDINGS_ENABLED=false");
        None
    };

    // Load pipeline configuration from environment
    let pipeline_config = Arc::new(PipelineConfig::from_env());
    let intel_config = Arc::new(sr_config::IntelConfig::from_env());
    info!("Pipeline config loaded (use PIPELINE_CONFIG_JSON or PIPELINE_* env vars to override)");

    // Load restricted airspace spatial index for aviation event annotation
    let airspace_index: SharedAirspaceIndex = {
        let airspace_path = std::env::var("AIRSPACE_DATA_PATH")
            .unwrap_or_else(|_| "static/data/restricted-airspace.json".to_string());
        match std::fs::read_to_string(&airspace_path) {
            Ok(json_str) => {
                let idx = AirspaceIndex::from_geojson(&json_str);
                info!(zones = idx.zone_count(), path = %airspace_path, "Airspace spatial index loaded");
                Arc::new(idx)
            }
            Err(e) => {
                warn!("Failed to load restricted airspace data from {}: {e} — airspace annotation disabled", airspace_path);
                Arc::new(AirspaceIndex::empty())
            }
        }
    };

    // Spawn the pipeline: ingest → correlate → enrich → publish
    let (publish_tx, summaries, analysis, metrics) =
        spawn_pipeline(
            event_tx.clone(),
            claude_client,
            ollama_client,
            budget.clone(),
            pool.clone(),
            embedding_model,
            shared_entity_resolver.clone(),
            shared_entity_graph.clone(),
            pipeline_config.clone(),
            airspace_index.clone(),
        );
    info!("Event pipeline started (correlator + enrichment + publisher)");

    // Background task: refresh anomaly baselines from continuous aggregate (hourly)
    routes::analytics::spawn_baseline_refresh(pool.clone());
    info!("Anomaly baseline refresh task started");

    let situations: state::SharedSituations = Arc::new(std::sync::RwLock::new(Vec::new()));
    let cameras: state::SharedCameras = Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()));

    // Background task: listen for situation updates from pipeline
    {
        let mut rx = publish_tx.subscribe();
        let situations = situations.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(sr_pipeline::PublishEvent::Situations { clusters }) => {
                        if let Ok(mut lock) = situations.write() {
                            *lock = clusters;
                        }
                    }
                    Ok(_) => {} // ignore other event types
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Situations subscriber lagged {n}");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // Background task: periodic camera discovery near active situations
    {
        let situations = situations.clone();
        let cameras = cameras.clone();
        tokio::spawn(async move {
            use std::collections::HashMap;
            use std::time::Instant;

            let http = reqwest::Client::new();
            // Track last search time per rough geo cell to avoid repeated searches
            let mut search_cache: HashMap<(i32, i32), Instant> = HashMap::new();
            let cache_ttl = std::time::Duration::from_secs(3600); // 1h cache

            let finder = match sr_sources::shodan::ShodanCameraFinder::new(http) {
                Ok(f) => f,
                Err(_) => {
                    info!("Shodan API key not set — camera discovery disabled");
                    return;
                }
            };

            let mut interval = tokio::time::interval(std::time::Duration::from_secs(15 * 60));
            interval.tick().await; // consume first tick
            // Wait 2 minutes before first scan to let situations accumulate
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;

            loop {
                interval.tick().await;

                let clusters = match situations.read() {
                    Ok(lock) => lock.clone(),
                    Err(_) => continue,
                };

                // Prune expired cache entries
                search_cache.retain(|_, v| v.elapsed() < cache_ttl);

                for cluster in &clusters {
                    if cluster.severity < Severity::Medium {
                        continue;
                    }
                    let (lat, lon) = match cluster.centroid {
                        Some(c) => c,
                        None => continue,
                    };

                    // Round to ~50km grid cell to avoid duplicate searches
                    let cell = ((lat * 2.0).round() as i32, (lon * 2.0).round() as i32);
                    if search_cache.contains_key(&cell) {
                        continue;
                    }

                    match finder.find_cameras(lat, lon, 50.0).await {
                        Ok(results) => {
                            search_cache.insert(cell, Instant::now());
                            if !results.is_empty() {
                                info!(
                                    cluster_id = %cluster.id,
                                    count = results.len(),
                                    "Found cameras near situation"
                                );
                                if let Ok(mut lock) = cameras.write() {
                                    lock.insert(cluster.id, results);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!("Camera search failed for cluster {}: {e}", cluster.id);
                            // Mark cell as searched to avoid rapid retries
                            search_cache.insert(cell, Instant::now());
                        }
                    }

                    // Brief pause between searches to respect rate limits
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        });
    }

    let state = AppState {
        db: pool,
        publish_tx,
        summaries,
        source_registry: registry,
        sse_event_counter: Arc::new(AtomicU64::new(0)),
        analysis,
        budget,
        situations,
        cameras,
        metrics,
        entity_resolver: shared_entity_resolver,
        entity_graph: shared_entity_graph,
        pipeline_config,
        intel_config,
    };

    // API routes
    let api = Router::new()
        .route("/api/events", get(routes::events::list_events))
        .route("/api/events/latest", get(routes::events::latest_events))
        .route("/api/events/geo", get(routes::events::events_geo))
        .route("/api/sse", get(routes::sse::sse_handler))
        .route("/api/sources", get(routes::sources::list_sources))
        .route(
            "/api/sources/{source_id}/config",
            get(routes::sources::get_source_config)
                .put(routes::sources::update_source_config),
        )
        .route(
            "/api/sources/{source_id}/toggle",
            post(routes::sources::toggle_source),
        )
        .route("/api/positions", get(routes::positions::list_positions))
        .route("/api/positions/{entity_id}/trail", get(routes::positions::get_position_trail))
        .route("/api/config", get(routes::config::get_app_config))
        .route("/api/config/pipeline", get(routes::config::get_pipeline_config))
        .route("/api/config/intel", get(routes::config::get_intel_config))
        .route("/api/stats", get(routes::events::event_stats))
        .route(
            "/api/pipeline/summaries",
            get(routes::pipeline::get_summaries),
        )
        .route(
            "/api/pipeline/metrics",
            get(routes::pipeline::get_pipeline_metrics),
        )
        // Intelligence layer
        .route("/api/intel/latest", get(routes::intel::get_latest_analysis))
        .route("/api/intel/budget", get(routes::intel::get_budget))
        // Translation stub (Haiku enrichment handles translation now)
        .route("/api/translate", post(routes::translate::translate))
        // Entities
        .route("/api/entities", get(routes::entities::list_entities))
        .route("/api/entities/state-changes", get(routes::entities::list_state_changes))
        .route("/api/entities/{id}", get(routes::entities::get_entity))
        // Situation clusters
        .route("/api/situations", get(routes::situations::list_situations))
        .route("/api/situations/{id}", get(routes::situations::get_situation))
        .route("/api/situations/{id}/narratives", get(routes::situations::get_situation_narratives))
        .route("/api/situations/{id}/events", get(routes::situations::get_situation_events))
        .route("/api/situations/{id}/cameras", get(routes::situations::get_situation_cameras))
        // Shodan proxy routes
        .route("/api/shodan/search", get(routes::shodan::search_shodan))
        .route("/api/shodan/host/{ip}", get(routes::shodan::host_lookup))
        .route("/api/shodan/alerts", get(routes::shodan::list_alerts))
        .route("/api/shodan/api-info", get(routes::shodan::api_info))
        .route("/api/shodan/scan", post(routes::shodan::submit_scan))
        .route("/api/shodan/discover", post(routes::shodan::trigger_discovery))
        // Analytics
        .route("/api/analytics/timeseries", get(routes::analytics::get_timeseries))
        .route("/api/analytics/anomalies", get(routes::analytics::get_anomalies))
        .route("/api/analytics/sources/health", get(routes::analytics::get_sources_health))
        // Search
        .route("/api/search", get(routes::search::search_events))
        .route("/api/search/similar", get(routes::search::search_similar))
        // Reports
        .route("/api/reports", get(routes::reports::list_reports))
        .route("/api/reports/{id}", get(routes::reports::get_report))
        // Alerts
        .route("/api/alerts/rules", get(routes::alerts::list_rules).post(routes::alerts::create_rule))
        .route("/api/alerts/rules/{id}", delete(routes::alerts::delete_rule))
        .route("/api/alerts/history", get(routes::alerts::get_history));

    // Static file serving for SvelteKit SPA
    let static_dir = std::env::var("STATIC_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            // Try relative to repo root first (running from repo root), then relative to backend/
            let repo_root = PathBuf::from("frontend/build");
            if repo_root.exists() {
                repo_root
            } else {
                PathBuf::from("../frontend/build")
            }
        });

    let app = Router::new()
        .merge(api)
        .merge(static_files::static_file_router(static_dir))
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let bind_addr: SocketAddr = std::env::var("BIND_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:3001".to_string())
        .parse()?;

    info!("Server listening on {bind_addr}");
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
