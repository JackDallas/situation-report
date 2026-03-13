use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{Datelike, DateTime, NaiveDate, Utc};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::warn;

use crate::gemini;
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

/// Tracks daily + monthly AI spend across all models. Thread-safe, DB-persisted.
///
/// In-memory atomics provide fast reads for budget gates. DB persistence
/// ensures counters survive container restarts.
pub struct BudgetManager {
    daily_cap_usd: f64,
    monthly_cap_usd: f64,
    haiku: ModelCounters,
    sonnet: ModelCounters,
    gemini_flash_lite: ModelCounters,
    gemini_flash: ModelCounters,
    /// When the current day's budget period started (resets daily)
    day_start: RwLock<DateTime<Utc>>,
    /// When the current month's budget period started (resets monthly)
    month_start: RwLock<DateTime<Utc>>,
    /// Cumulative Gemini spend this month in micro-USD (atomic for lock-free reads)
    gemini_month_spend_micro_usd: AtomicU64,
    /// DB pool for persisting budget. None if no DB available.
    pool: Option<PgPool>,
}

impl BudgetManager {
    /// Create a new budget manager. Reads INTEL_DAILY_BUDGET_USD (default $10)
    /// and INTEL_MONTHLY_BUDGET_USD (default $30) from env.
    pub fn from_env() -> Arc<Self> {
        let daily_cap = std::env::var("INTEL_DAILY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);
        let monthly_cap = std::env::var("INTEL_MONTHLY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(30.0);

        Arc::new(Self {
            daily_cap_usd: daily_cap,
            monthly_cap_usd: monthly_cap,
            haiku: ModelCounters::default(),
            sonnet: ModelCounters::default(),
            gemini_flash_lite: ModelCounters::default(),
            gemini_flash: ModelCounters::default(),
            day_start: RwLock::new(Utc::now()),
            month_start: RwLock::new(Utc::now()),
            gemini_month_spend_micro_usd: AtomicU64::new(0),
            pool: None,
        })
    }

    /// Create a budget manager that persists to DB, loading today's counters on startup.
    pub async fn from_db(pool: PgPool) -> Arc<Self> {
        let daily_cap = std::env::var("INTEL_DAILY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(10.0);
        let monthly_cap = std::env::var("INTEL_MONTHLY_BUDGET_USD")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(30.0);

        let manager = Arc::new(Self {
            daily_cap_usd: daily_cap,
            monthly_cap_usd: monthly_cap,
            haiku: ModelCounters::default(),
            sonnet: ModelCounters::default(),
            gemini_flash_lite: ModelCounters::default(),
            gemini_flash: ModelCounters::default(),
            day_start: RwLock::new(Utc::now()),
            month_start: RwLock::new(Utc::now()),
            gemini_month_spend_micro_usd: AtomicU64::new(0),
            pool: Some(pool),
        });

        // Load today's counters from DB
        if let Err(e) = manager.load_from_db().await {
            warn!(error = %e, "Failed to load budget from DB — starting from zero");
        }

        manager
    }

    /// Helper: get the first day of the current UTC month.
    fn current_month_start() -> NaiveDate {
        let now = Utc::now();
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1).unwrap()
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

        // Load monthly Gemini spend
        let month_start = Self::current_month_start();
        let monthly_micro = sr_sources::db::queries::load_gemini_monthly_spend(pool, month_start).await?;
        if monthly_micro > 0 {
            self.gemini_month_spend_micro_usd.store(monthly_micro as u64, Ordering::Relaxed);
            tracing::info!(
                gemini_month_spend_usd = format!("{:.4}", monthly_micro as f64 / 1_000_000.0),
                monthly_cap_usd = self.monthly_cap_usd,
                "Gemini monthly spend loaded from DB"
            );
        }

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

    /// Record token usage for a Gemini call and update monthly spend.
    pub fn record_gemini(&self, model: gemini::GeminiModel, response: &gemini::GeminiResponse) {
        let counters = match model {
            gemini::GeminiModel::FlashLite => &self.gemini_flash_lite,
            gemini::GeminiModel::Flash => &self.gemini_flash,
        };
        counters.record(
            response.usage.prompt_token_count,
            response.usage.candidates_token_count,
            response.usage.cached_content_token_count,
        );
        // Track monthly spend in micro-USD for atomicity
        let cost_micro = (response.cost_usd() * 1_000_000.0) as u64;
        let new_total = self.gemini_month_spend_micro_usd.fetch_add(cost_micro, Ordering::Relaxed) + cost_micro;

        // Persist to DB periodically (~every $0.10 of spend)
        if new_total % 100_000 < cost_micro {
            self.persist_monthly_spend(new_total);
        }
    }

    /// Fire-and-forget persist of monthly Gemini spend to DB.
    fn persist_monthly_spend(&self, spend_micro_usd: u64) {
        if let Some(pool) = &self.pool {
            let pool = pool.clone();
            let month_start = Self::current_month_start();
            tokio::spawn(async move {
                if let Err(e) = sr_sources::db::queries::persist_gemini_monthly_spend(
                    &pool,
                    month_start,
                    spend_micro_usd as i64,
                ).await {
                    warn!(error = %e, "Failed to persist Gemini monthly spend to DB");
                }
            });
        }
    }

    /// Check if we can afford a Gemini call (monthly cap not exceeded).
    pub async fn can_afford_gemini(&self) -> bool {
        self.maybe_reset_month().await;
        self.gemini_month_spend_usd() < self.monthly_cap_usd
    }

    /// Current Gemini monthly spend in USD.
    pub fn gemini_month_spend_usd(&self) -> f64 {
        self.gemini_month_spend_micro_usd.load(Ordering::Relaxed) as f64 / 1_000_000.0
    }

    /// Current total spend today in USD (Claude only — Gemini tracked separately by month).
    pub fn current_spend(&self) -> f64 {
        self.haiku.cost(&HAIKU_PRICING) + self.sonnet.cost(&SONNET_PRICING)
    }

    /// Current total Gemini spend today in USD.
    fn gemini_daily_spend(&self) -> f64 {
        let fl_pricing = gemini::pricing_for(gemini::GeminiModel::FlashLite);
        let f_pricing = gemini::pricing_for(gemini::GeminiModel::Flash);
        let fl_cost = self.gemini_flash_lite.total_input() as f64 / 1e6 * fl_pricing.input_per_m
            + self.gemini_flash_lite.total_output() as f64 / 1e6 * fl_pricing.output_per_m;
        let f_cost = self.gemini_flash.total_input() as f64 / 1e6 * f_pricing.input_per_m
            + self.gemini_flash.total_output() as f64 / 1e6 * f_pricing.output_per_m;
        fl_cost + f_cost
    }

    /// Get current budget status for the API endpoint.
    pub async fn status(&self) -> BudgetStatus {
        self.maybe_reset_day().await;
        self.maybe_reset_month().await;
        let spent = self.current_spend() + self.gemini_daily_spend();
        BudgetStatus {
            daily_budget_usd: self.daily_cap_usd,
            spent_today_usd: spent,
            remaining_usd: (self.daily_cap_usd - spent).max(0.0),
            haiku_tokens_today: self.haiku.total_input() + self.haiku.total_output(),
            sonnet_tokens_today: self.sonnet.total_input() + self.sonnet.total_output(),
            budget_exhausted: spent >= self.daily_cap_usd,
            degraded: !self.can_afford_sonnet_sync(),
            gemini_spent_month_usd: self.gemini_month_spend_usd(),
            gemini_month_limit_usd: self.monthly_cap_usd,
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
            self.gemini_flash_lite.reset();
            self.gemini_flash.reset();
            *self.day_start.write().await = now;
        }
    }

    /// Reset monthly Gemini counter if a new UTC month has started.
    async fn maybe_reset_month(&self) {
        let now = Utc::now();
        let month_start = *self.month_start.read().await;
        if now.date_naive().month() != month_start.date_naive().month()
            || now.date_naive().year() != month_start.date_naive().year()
        {
            self.gemini_month_spend_micro_usd.store(0, Ordering::Relaxed);
            *self.month_start.write().await = now;
            // Persist the new month's zero start to DB
            self.persist_monthly_spend(0);
        }
    }
}
