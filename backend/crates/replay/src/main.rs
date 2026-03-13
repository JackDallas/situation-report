//! sr-replay: CLI tool for replaying events through the situation clustering algorithm.
//!
//! Primary workflow:
//!   sr-replay run --database-url $DATABASE_URL
//!   sr-replay golden -i baseline.json -o golden.json
//!   sr-replay eval -i candidate.json -g golden.json
//!
//! Commands:
//!   run     — replay events from the database directly
//!   golden  — auto-generate a golden expectation file from replay results
//!   eval    — score a replay run against a golden file
//!   compare — diff two replay results side-by-side
//!   export  — dump events to JSONL (for archival)
//!   run-file — replay a JSONL dataset

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use clap::{Parser, Subcommand};
use tracing::info;

use sr_config::PipelineConfig;
use sr_intel::{BudgetManager, ClaudeClient, OllamaClient};
use sr_pipeline::replay::{
    GoldenFile, ReplayComparison, ReplayConfig, ReplayDataset, ReplayEvent, ReplayHarness,
    ReplayMetrics, ScoreCard,
};
use sr_sources::db::queries::{
    count_replay_events, query_replay_events, query_replay_events_filtered,
};

#[derive(Parser)]
#[command(name = "sr-replay", about = "Situation Room replay & testing tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Replay events directly from the database (primary workflow).
    Run {
        /// Database URL (or set DATABASE_URL env var).
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,

        /// Start of time range (ISO 8601). Default: 72 hours ago.
        #[arg(long)]
        since: Option<DateTime<Utc>>,

        /// End of time range (ISO 8601). Default: now.
        #[arg(long)]
        until: Option<DateTime<Utc>>,

        /// Maximum events to replay. Default: 2,000,000.
        #[arg(long, default_value = "2000000")]
        limit: i64,

        /// Output metrics file (.json). If omitted, prints to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Pipeline config file (JSON). Uses defaults if omitted.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Snapshot interval in minutes. Default: 15.
        #[arg(long, default_value = "15")]
        snapshot_interval: i64,

        /// Skip high-volume noise types (flight/vessel positions, shodan banners).
        /// Much faster — these are rejected by the pipeline anyway.
        #[arg(long, default_value = "true")]
        filter: bool,

        /// Human-readable label for this run (e.g., "baseline", "wider-merge").
        #[arg(long)]
        label: Option<String>,

        /// Enable AI processing (enrichment, titles, narratives).
        /// Requires OLLAMA_URL and/or ANTHROPIC_API_KEY env vars.
        #[arg(long, default_value = "false")]
        ai: bool,
    },

    /// Auto-generate a golden expectation file from replay results.
    Golden {
        /// Input replay metrics file (.json).
        #[arg(short, long)]
        input: PathBuf,

        /// Output golden file (.json).
        #[arg(short, long)]
        output: PathBuf,

        /// Name for this golden file.
        #[arg(long, default_value = "baseline")]
        name: String,

        /// Description.
        #[arg(long, default_value = "Auto-generated from baseline replay")]
        description: String,
    },

    /// Score a replay run against a golden expectation file.
    Eval {
        /// Input replay metrics file (.json).
        #[arg(short, long)]
        input: PathBuf,

        /// Golden expectation file (.json).
        #[arg(short, long)]
        golden: PathBuf,
    },

    /// Compare two replay results and show differences.
    Compare {
        /// Baseline metrics file (.json).
        #[arg(short, long)]
        baseline: PathBuf,

        /// Candidate metrics file (.json).
        #[arg(short, long)]
        candidate: PathBuf,

        /// Label for baseline.
        #[arg(long, default_value = "baseline")]
        baseline_label: String,

        /// Label for candidate.
        #[arg(long, default_value = "candidate")]
        candidate_label: String,
    },

    /// Replay a previously exported JSONL dataset file.
    RunFile {
        /// Input dataset file (.jsonl).
        #[arg(short, long)]
        input: PathBuf,

        /// Output metrics file (.json). If omitted, prints to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Pipeline config file (JSON). Uses defaults if omitted.
        #[arg(long)]
        config: Option<PathBuf>,

        /// Snapshot interval in minutes. Default: 15.
        #[arg(long, default_value = "15")]
        snapshot_interval: i64,

        /// Enable AI processing (enrichment, titles, narratives).
        /// Requires OLLAMA_URL and/or ANTHROPIC_API_KEY env vars.
        #[arg(long, default_value = "false")]
        ai: bool,
    },

    /// Export events from the database to a replay dataset file.
    Export {
        /// Output file path (.jsonl).
        #[arg(short, long)]
        output: PathBuf,

        /// Start of time range (ISO 8601). Default: 72 hours ago.
        #[arg(long)]
        since: Option<DateTime<Utc>>,

        /// End of time range (ISO 8601). Default: now.
        #[arg(long)]
        until: Option<DateTime<Utc>>,

        /// Maximum events to export. Default: 1,000,000.
        #[arg(long, default_value = "1000000")]
        limit: i64,

        /// Human-readable name for this dataset.
        #[arg(long, default_value = "unnamed")]
        name: String,

        /// Database URL (or set DATABASE_URL env var).
        #[arg(long, env = "DATABASE_URL")]
        database_url: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sr_replay=info,sr_pipeline=info".into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            database_url,
            since,
            until,
            limit,
            output,
            config,
            snapshot_interval,
            filter,
            label,
            ai,
        } => {
            cmd_run_db(
                database_url,
                since,
                until,
                limit,
                output,
                config,
                snapshot_interval,
                filter,
                label,
                ai,
            )
            .await
        }
        Commands::Golden {
            input,
            output,
            name,
            description,
        } => cmd_golden(input, output, name, description),
        Commands::Eval { input, golden } => cmd_eval(input, golden),
        Commands::Compare {
            baseline,
            candidate,
            baseline_label,
            candidate_label,
        } => cmd_compare(baseline, candidate, baseline_label, candidate_label),
        Commands::RunFile {
            input,
            output,
            config,
            snapshot_interval,
            ai,
        } => cmd_run_file(input, output, config, snapshot_interval, ai).await,
        Commands::Export {
            output,
            since,
            until,
            limit,
            name,
            database_url,
        } => cmd_export(output, since, until, limit, name, database_url).await,
    }
}

// ── Commands ─────────────────────────────────────────────────────────────

async fn cmd_run_db(
    database_url: String,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: i64,
    output: Option<PathBuf>,
    config_path: Option<PathBuf>,
    snapshot_interval: i64,
    filter: bool,
    label: Option<String>,
    ai: bool,
) -> Result<()> {
    let since = since.unwrap_or_else(|| Utc::now() - Duration::hours(72));
    let until = until.unwrap_or_else(Utc::now);

    info!(%since, %until, filter, ai, "Connecting to database");
    let pool = sr_sources::db::connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    let total = count_replay_events(&pool, since, until).await?;
    info!(total, "Total events in time range");

    // Fetch events (filtered or unfiltered)
    let events = if filter {
        info!("Using pipeline-relevant filter (skipping positions/banners)");
        query_replay_events_filtered(&pool, since, until, limit).await?
    } else {
        query_replay_events(&pool, since, until, limit).await?
    };
    info!(fetched = events.len(), "Events loaded for replay");

    let replay_events: Vec<ReplayEvent> = events.into_iter().map(ReplayEvent::from).collect();

    let dataset = ReplayDataset::from_events(
        format!("db-replay-{}", Utc::now().format("%Y%m%dT%H%M%S")),
        replay_events,
        since,
        until,
        None,
    );

    let pipeline_config = load_config(config_path)?;
    let replay_config = ReplayConfig {
        snapshot_interval: Duration::minutes(snapshot_interval),
        flush_pending: true,
        ai_enabled: ai,
    };

    let mut harness = if ai {
        let ollama = OllamaClient::from_env();
        let claude = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .map(|key| Arc::new(ClaudeClient::new(key)));
        let budget = BudgetManager::from_env();
        if ollama.is_none() && claude.is_none() {
            anyhow::bail!("--ai requires OLLAMA_URL and/or ANTHROPIC_API_KEY env vars");
        }
        info!(
            ollama = ollama.is_some(),
            claude = claude.is_some(),
            "AI clients initialized for replay"
        );
        ReplayHarness::with_ai(pipeline_config, replay_config, ollama, claude, budget)
    } else {
        ReplayHarness::new(pipeline_config, replay_config)
    };
    let mut metrics = harness.run(&dataset).await;

    // Tag with code version and label for reproducibility
    metrics.git_hash = detect_git_hash();
    metrics.label = label;

    output_metrics(&metrics, output)?;
    print_metrics_summary(&metrics);

    Ok(())
}

fn cmd_golden(
    input: PathBuf,
    output: PathBuf,
    name: String,
    description: String,
) -> Result<()> {
    let metrics: ReplayMetrics = serde_json::from_str(
        &std::fs::read_to_string(&input)
            .with_context(|| format!("Failed to read {}", input.display()))?,
    )
    .context("Failed to parse replay metrics")?;

    let golden = sr_pipeline::replay::generate_golden(&metrics, name, description);

    let json = serde_json::to_string_pretty(&golden)?;
    std::fs::write(&output, &json)
        .with_context(|| format!("Failed to write {}", output.display()))?;

    info!(
        path = %output.display(),
        expectations = golden.expectations.len(),
        anti_expectations = golden.anti_expectations.len(),
        "Golden file generated"
    );

    eprintln!("\nGenerated {} expectations from baseline:", golden.expectations.len());
    for exp in &golden.expectations {
        eprintln!("  [w={:.1}] {}", exp.weight, exp.title_pattern);
    }
    eprintln!("\n{} anti-patterns:", golden.anti_expectations.len());
    for anti in &golden.anti_expectations {
        eprintln!("  [w={:.1}] {} — {}", anti.weight, anti.title_pattern, anti.reason);
    }
    eprintln!("\nThresholds:");
    if let Some(max) = golden.thresholds.max_top_level {
        eprintln!("  max_top_level: {}", max);
    }
    if let Some(cert) = golden.thresholds.min_avg_certainty {
        eprintln!("  min_avg_certainty: {:.2}", cert);
    }
    if let Some(ratio) = golden.thresholds.min_multi_source_ratio {
        eprintln!("  min_multi_source_ratio: {:.2}", ratio);
    }

    eprintln!("\nReview and edit {} before using as a test fixture.", output.display());

    Ok(())
}

fn cmd_eval(input: PathBuf, golden_path: PathBuf) -> Result<()> {
    let metrics: ReplayMetrics = serde_json::from_str(
        &std::fs::read_to_string(&input)
            .with_context(|| format!("Failed to read {}", input.display()))?,
    )
    .context("Failed to parse replay metrics")?;

    let golden: GoldenFile = serde_json::from_str(
        &std::fs::read_to_string(&golden_path)
            .with_context(|| format!("Failed to read {}", golden_path.display()))?,
    )
    .context("Failed to parse golden file")?;

    let card = ScoreCard::evaluate(&golden, &metrics);

    // Print score card
    eprintln!("\n=== Evaluation: {} ===\n", card.golden_name);

    eprintln!("Expectations:");
    for r in &card.expectations {
        let status = if r.passed { "PASS" } else { "FAIL" };
        let matched = r
            .matched_title
            .as_deref()
            .unwrap_or("(no match)");
        eprintln!("  [{}] {} -> {}", status, r.title_pattern, matched);
        for d in &r.details {
            eprintln!("        {}", d);
        }
    }

    eprintln!("\nAnti-patterns:");
    for r in &card.anti_expectations {
        let status = if r.passed { "PASS" } else { "FAIL" };
        eprintln!("  [{}] {} — {}", status, r.title_pattern, r.reason);
        for v in &r.violations {
            eprintln!("        violation: {}", v);
        }
    }

    eprintln!("\nThresholds:");
    for r in &card.thresholds {
        let status = if r.passed { "PASS" } else { "FAIL" };
        eprintln!(
            "  [{}] {}: {:.2} (threshold: {:.2})",
            status, r.name, r.actual, r.threshold
        );
    }

    eprintln!(
        "\n=== Score: {:.1}% ({}/{} passed) ===\n",
        card.score * 100.0,
        card.passed,
        card.total,
    );

    // Print JSON to stdout for piping
    println!("{}", serde_json::to_string_pretty(&card)?);

    // Exit with non-zero if score is below 100%
    if card.score < 1.0 {
        std::process::exit(1);
    }

    Ok(())
}

fn cmd_compare(
    baseline_path: PathBuf,
    candidate_path: PathBuf,
    baseline_label: String,
    candidate_label: String,
) -> Result<()> {
    let baseline: ReplayMetrics = serde_json::from_str(
        &std::fs::read_to_string(&baseline_path)
            .with_context(|| format!("Failed to read {}", baseline_path.display()))?,
    )
    .context("Failed to parse baseline metrics")?;

    let candidate: ReplayMetrics = serde_json::from_str(
        &std::fs::read_to_string(&candidate_path)
            .with_context(|| format!("Failed to read {}", candidate_path.display()))?,
    )
    .context("Failed to parse candidate metrics")?;

    let comparison = ReplayComparison::compare(
        baseline_label.clone(),
        &baseline,
        candidate_label.clone(),
        &candidate,
    );

    eprintln!("\n=== Replay Comparison: {baseline_label} vs {candidate_label} ===");
    if let (Some(bh), Some(ch)) = (&baseline.git_hash, &candidate.git_hash) {
        eprintln!("Git:         {} -> {}", bh, ch);
    }
    eprintln!();
    eprintln!(
        "Clusters:    {} -> {} ({:+})",
        baseline.raw_cluster_count,
        candidate.raw_cluster_count,
        comparison.cluster_count_delta
    );
    eprintln!(
        "Certainty:   {:.2} -> {:.2} ({:+.2})",
        baseline.avg_certainty, candidate.avg_certainty, comparison.avg_certainty_delta
    );
    eprintln!(
        "Peak:        {} -> {} ({:+})",
        baseline.peak_cluster_count,
        candidate.peak_cluster_count,
        comparison.peak_cluster_delta
    );

    if !comparison.lost_titles.is_empty() {
        eprintln!("\nLost situations:");
        for t in &comparison.lost_titles {
            eprintln!("  - {t}");
        }
    }
    if !comparison.new_titles.is_empty() {
        eprintln!("\nNew situations:");
        for t in &comparison.new_titles {
            eprintln!("  + {t}");
        }
    }
    if !comparison.common_titles.is_empty() {
        eprintln!("\nUnchanged situations:");
        for t in &comparison.common_titles {
            eprintln!("  = {t}");
        }
    }

    println!("{}", serde_json::to_string_pretty(&comparison)?);

    Ok(())
}

async fn cmd_run_file(
    input: PathBuf,
    output: Option<PathBuf>,
    config_path: Option<PathBuf>,
    snapshot_interval: i64,
    ai: bool,
) -> Result<()> {
    let dataset = load_dataset(&input)?;
    let pipeline_config = load_config(config_path)?;

    let replay_config = ReplayConfig {
        snapshot_interval: Duration::minutes(snapshot_interval),
        flush_pending: true,
        ai_enabled: ai,
    };

    let mut harness = if ai {
        let ollama = OllamaClient::from_env();
        let claude = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .map(|key| Arc::new(ClaudeClient::new(key)));
        let budget = BudgetManager::from_env();
        if ollama.is_none() && claude.is_none() {
            anyhow::bail!("--ai requires OLLAMA_URL and/or ANTHROPIC_API_KEY env vars");
        }
        info!(
            ollama = ollama.is_some(),
            claude = claude.is_some(),
            "AI clients initialized for replay"
        );
        ReplayHarness::with_ai(pipeline_config, replay_config, ollama, claude, budget)
    } else {
        ReplayHarness::new(pipeline_config, replay_config)
    };
    let metrics = harness.run(&dataset).await;

    output_metrics(&metrics, output)?;
    print_metrics_summary(&metrics);

    Ok(())
}

async fn cmd_export(
    output: PathBuf,
    since: Option<DateTime<Utc>>,
    until: Option<DateTime<Utc>>,
    limit: i64,
    name: String,
    database_url: String,
) -> Result<()> {
    let since = since.unwrap_or_else(|| Utc::now() - Duration::hours(72));
    let until = until.unwrap_or_else(Utc::now);

    info!(%since, %until, "Connecting to database");
    let pool = sr_sources::db::connect(&database_url)
        .await
        .context("Failed to connect to database")?;

    let total = count_replay_events(&pool, since, until).await?;
    info!(total, "Events in range");

    let events = query_replay_events(&pool, since, until, limit).await?;
    info!(fetched = events.len(), "Events fetched");

    let replay_events: Vec<ReplayEvent> = events.into_iter().map(ReplayEvent::from).collect();

    let git_hash = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());

    let dataset = ReplayDataset::from_events(name, replay_events, since, until, git_hash);

    let file = std::fs::File::create(&output)
        .with_context(|| format!("Failed to create {}", output.display()))?;
    let mut writer = BufWriter::new(file);

    serde_json::to_writer(&mut writer, &dataset.metadata)?;
    writeln!(writer)?;

    for event in &dataset.events {
        serde_json::to_writer(&mut writer, event)?;
        writeln!(writer)?;
    }
    writer.flush()?;

    info!(
        path = %output.display(),
        events = dataset.metadata.event_count,
        "Dataset exported"
    );

    let mut sources: Vec<_> = dataset.metadata.source_counts.iter().collect();
    sources.sort_by(|a, b| b.1.cmp(a.1));
    for (source, count) in sources {
        info!("  {source}: {count}");
    }

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn load_config(path: Option<PathBuf>) -> Result<PipelineConfig> {
    if let Some(path) = path {
        let data = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        serde_json::from_str(&data).context("Failed to parse pipeline config")
    } else {
        Ok(PipelineConfig::default())
    }
}

fn load_dataset(input: &PathBuf) -> Result<ReplayDataset> {
    let file = std::fs::File::open(input)
        .with_context(|| format!("Failed to open {}", input.display()))?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let meta_line = lines
        .next()
        .context("Empty dataset file")?
        .context("Failed to read metadata line")?;
    let metadata: sr_pipeline::replay::ReplayMetadata =
        serde_json::from_str(&meta_line).context("Failed to parse metadata")?;

    let mut events = Vec::with_capacity(metadata.event_count);
    for line in lines {
        let line = line?;
        if line.is_empty() {
            continue;
        }
        let event: ReplayEvent =
            serde_json::from_str(&line).context("Failed to parse event line")?;
        events.push(event);
    }

    info!(name = metadata.name, events = events.len(), "Dataset loaded");

    Ok(ReplayDataset { metadata, events })
}

fn output_metrics(metrics: &ReplayMetrics, output: Option<PathBuf>) -> Result<()> {
    let json = serde_json::to_string_pretty(metrics)?;
    if let Some(out_path) = output {
        std::fs::write(&out_path, &json)
            .with_context(|| format!("Failed to write {}", out_path.display()))?;
        info!(path = %out_path.display(), "Metrics written");
    } else {
        println!("{json}");
    }
    Ok(())
}

fn detect_git_hash() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn print_metrics_summary(metrics: &ReplayMetrics) {
    eprintln!("\n=== Replay Summary ===");
    if let Some(ref label) = metrics.label {
        eprintln!("Label:       {}", label);
    }
    if let Some(ref hash) = metrics.git_hash {
        eprintln!("Git:         {}", hash);
    }
    eprintln!("Events:      {} total, {} accepted", metrics.total_events, metrics.events_accepted);
    eprintln!("Raw clusters:    {}", metrics.raw_cluster_count);
    eprintln!("Filtered clusters: {}", metrics.final_cluster_count);
    eprintln!("Peak:        {}", metrics.peak_cluster_count);
    eprintln!("Certainty:   {:.2}", metrics.avg_certainty);
    eprintln!("Titled:      {}", metrics.titled_clusters);
    eprintln!("Duration:    {}ms", metrics.replay_duration_ms);
    eprintln!("Snapshots:   {}", metrics.snapshots.len());

    if let Some(final_snap) = metrics.snapshots.last() {
        eprintln!("\nFinal situations:");
        let mut clusters: Vec<_> = final_snap
            .clusters
            .iter()
            .filter(|c| c.parent_id.is_none())
            .collect();
        clusters.sort_by(|a, b| b.event_count.cmp(&a.event_count));
        for c in clusters {
            eprintln!(
                "  [{:.0}%] {} ({} events, {} sources)",
                c.certainty * 100.0,
                c.title,
                c.event_count,
                c.source_count,
            );
        }
    }
}
