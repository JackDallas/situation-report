use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::warn;

use crate::types::BudgetStatus;

/// Per-model pricing in USD per million tokens.
struct ModelPricing {
    input_per_m: f64,
    output_per_m: f64,
    cache_read_per_m: f64,
}

const HAIKU_PRICING: ModelPricing = ModelPricing {
    input_per_m: 1.0,
    output_per_m: 5.0,
    cache_read_per_m: 0.10,
};

const SONNET_PRICING: ModelPricing = ModelPricing {
    input_per_m: 3.0,
    output_per_m: 15.0,
    cache_read_per_m: 0.30,
};

/// Token counters for a specific model.
#[derive(Debug, Default)]
struct ModelCounters {
    input_tokens: AtomicU64,
    output_tokens: AtomicU64,
    cache_read_tokens: AtomicU64,
}

impl ModelCounters {
    fn total_input(&self) -> u64 {
        self.input_tokens.load(Ordering::Relaxed)
    }

    fn total_output(&self) -> u64 {
        self.output_tokens.load(Ordering::Relaxed)
    }

    fn total_cache_read(&self) -> u64 {
        self.cache_read_tokens.load(Ordering::Relaxed)
    }

    fn record(&self, input: u32, output: u32, cache_read: u32) {
        self.input_tokens.fetch_add(input as u64, Ordering::Relaxed);
        self.output_tokens.fetch_add(output as u64, Ordering::Relaxed);
        self.cache_read_tokens.fetch_add(cache_read as u64, Ordering::Relaxed);
    }

    fn cost(&self, pricing: &ModelPricing) -> f64 {
        let input = self.total_input() as f64 / 1_000_000.0;
        let output = self.total_output() as f64 / 1_000_000.0;
        let cache_read = self.total_cache_read() as f64 / 1_000_000.0;

        input * pricing.input_per_m + output * pricing.output_per_m + cache_read * pricing.cache_read_per_m
    }

    fn reset(&self) {
        self.input_tokens.store(0, Ordering::Relaxed);
        self.output_tokens.store(0, Ordering::Relaxed);
        self.cache_read_tokens.store(0, Ordering::Relaxed);
    }

    fn init(&self, input: u64, output: u64, cache_read: u64) {
        self.input_tokens.store(input, Ordering::Relaxed);
        self.output_tokens.store(output, Ordering::Relaxed);
        self.cache_read_tokens.store(cache_read, Ordering::Relaxed);
    }
}

/// Tracks daily AI spend across all models. Thread-safe, DB-persisted.
///
/// In-memory atomics provide fast reads for budget gates. DB persistence
/// ensures counters survive container restarts.
pub struct BudgetManager {
    daily_cap_usd: f64,
    haiku: ModelCounters,
    sonnet: ModelCounters,
    /// When the current day's budget period started (resets daily)
    day_start: RwLock<DateTime<Utc>>,
    /// DB pool for persisting budget. None if no DB available.
    pool: Option<PgPool>,
}

impl BudgetManager {
    /// Create a new budget manager. Reads INTEL_DAILY_BUDGET_USD from env (default $10).
    pub fn from_env() -> Arc<Self> {
        let cap = std::env::var("INTEL_DAILY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);

        Arc::new(Self {
            daily_cap_usd: cap,
            haiku: ModelCounters::default(),
            sonnet: ModelCounters::default(),
            day_start: RwLock::new(Utc::now()),
            pool: None,
        })
    }

    /// Create a budget manager that persists to DB, loading today's counters on startup.
    pub async fn from_db(pool: PgPool) -> Arc<Self> {
        let cap = std::env::var("INTEL_DAILY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);

        let manager = Arc::new(Self {
            daily_cap_usd: cap,
            haiku: ModelCounters::default(),
            sonnet: ModelCounters::default(),
            day_start: RwLock::new(Utc::now()),
            pool: Some(pool),
        });

        // Load today's counters from DB
        if let Err(e) = manager.load_from_db().await {
            warn!(error = %e, "Failed to load budget from DB — starting from zero");
        }

        manager
    }

    /// Load today's counters from the DB into in-memory atomics.
    async fn load_from_db(&self) -> anyhow::Result<()> {
        let pool = self.pool.as_ref().ok_or_else(|| anyhow::anyhow!("no DB pool"))?;
        let row = sr_sources::db::queries::load_today_budget(pool).await?;
        self.haiku.init(
            row.haiku_input_tokens as u64,
            row.haiku_output_tokens as u64,
            row.haiku_cache_read_tokens as u64,
        );
        self.sonnet.init(
            row.sonnet_input_tokens as u64,
            row.sonnet_output_tokens as u64,
            row.sonnet_cache_read_tokens as u64,
        );
        let spent = self.current_spend();
        if spent > 0.0 {
            tracing::info!(
                spent_today_usd = format!("{:.4}", spent),
                daily_cap_usd = self.daily_cap_usd,
                "Budget loaded from DB — resuming today's spend"
            );
        }
        Ok(())
    }

    /// Persist a token increment to the DB (fire-and-forget).
    fn persist_increment(
        &self,
        haiku_input: u32,
        haiku_output: u32,
        haiku_cache_read: u32,
        sonnet_input: u32,
        sonnet_output: u32,
        sonnet_cache_read: u32,
    ) {
        if let Some(pool) = &self.pool {
            let pool = pool.clone();
            tokio::spawn(async move {
                if let Err(e) = sr_sources::db::queries::record_budget_tokens(
                    &pool,
                    haiku_input as i64,
                    haiku_output as i64,
                    haiku_cache_read as i64,
                    sonnet_input as i64,
                    sonnet_output as i64,
                    sonnet_cache_read as i64,
                ).await {
                    warn!(error = %e, "Failed to persist budget tokens to DB");
                }
            });
        }
    }

    /// Check if we can afford a Haiku call (cheaper tier).
    pub async fn can_afford_haiku(&self) -> bool {
        self.maybe_reset_day().await;
        self.current_spend() < self.daily_cap_usd
    }

    /// Check if we can afford a Sonnet call (more expensive tier).
    /// Returns false if spend > 80% of budget (save headroom for Haiku).
    pub async fn can_afford_sonnet(&self) -> bool {
        self.maybe_reset_day().await;
        self.current_spend() < self.daily_cap_usd * 0.8
    }

    /// Record token usage for a Haiku call.
    pub fn record_haiku(&self, input: u32, output: u32, cache_read: u32) {
        self.haiku.record(input, output, cache_read);
        self.persist_increment(input, output, cache_read, 0, 0, 0);
    }

    /// Record token usage for a Sonnet call.
    pub fn record_sonnet(&self, input: u32, output: u32, cache_read: u32) {
        self.sonnet.record(input, output, cache_read);
        self.persist_increment(0, 0, 0, input, output, cache_read);
    }

    /// Current total spend today in USD.
    pub fn current_spend(&self) -> f64 {
        self.haiku.cost(&HAIKU_PRICING) + self.sonnet.cost(&SONNET_PRICING)
    }

    /// Get current budget status for the API endpoint.
    pub async fn status(&self) -> BudgetStatus {
        self.maybe_reset_day().await;
        let spent = self.current_spend();
        BudgetStatus {
            daily_budget_usd: self.daily_cap_usd,
            spent_today_usd: spent,
            remaining_usd: (self.daily_cap_usd - spent).max(0.0),
            haiku_tokens_today: self.haiku.total_input() + self.haiku.total_output(),
            sonnet_tokens_today: self.sonnet.total_input() + self.sonnet.total_output(),
            budget_exhausted: spent >= self.daily_cap_usd,
            degraded: !self.can_afford_sonnet_sync(),
        }
    }

    /// Synchronous check (doesn't reset day) — used internally.
    fn can_afford_sonnet_sync(&self) -> bool {
        self.current_spend() < self.daily_cap_usd * 0.8
    }

    /// Reset counters if a new UTC day has started.
    async fn maybe_reset_day(&self) {
        let now = Utc::now();
        let day_start = *self.day_start.read().await;
        if now.date_naive() != day_start.date_naive() {
            self.haiku.reset();
            self.sonnet.reset();
            *self.day_start.write().await = now;
        }
    }
}
