use std::sync::atomic::Ordering;

use axum::extract::State;
use axum::Json;
use sr_pipeline::{GpuState, Summary};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tracing::{info, warn};

use crate::state::AppState;

/// GET /api/pipeline/summaries — current high-volume type summaries (for dashboard stats)
pub async fn get_summaries(
    State(state): State<AppState>,
) -> Json<Vec<Summary>> {
    let summaries = state
        .summaries
        .read()
        .map(|lock| lock.values().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    Json(summaries)
}

/// GET /api/pipeline/metrics — atomic pipeline throughput counters
pub async fn get_pipeline_metrics(
    State(state): State<AppState>,
) -> axum::Json<serde_json::Value> {
    let m = &state.metrics;
    axum::Json(serde_json::json!({
        "events_ingested": m.events_ingested.load(Ordering::Relaxed),
        "events_correlated": m.events_correlated.load(Ordering::Relaxed),
        "events_enriched": m.events_enriched.load(Ordering::Relaxed),
        "events_published": m.events_published.load(Ordering::Relaxed),
        "events_filtered": m.events_filtered.load(Ordering::Relaxed),
        "incidents_created": m.incidents_created.load(Ordering::Relaxed),
        "gpu_paused": m.is_gpu_paused(),
        "gpu_state": m.gpu_state().as_str(),
    }))
}

/// Send an HTTP request to the Docker daemon via the Unix socket.
async fn docker_post(path: &str) -> anyhow::Result<u16> {
    let mut stream = UnixStream::connect("/var/run/docker.sock").await?;
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n"
    );
    stream.write_all(req.as_bytes()).await?;
    let mut buf = vec![0u8; 1024];
    let n = stream.read(&mut buf).await?;
    let response = String::from_utf8_lossy(&buf[..n]);
    // Parse HTTP status code from first line: "HTTP/1.1 204 No Content"
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(500);
    Ok(status)
}

/// Check if a Docker container is running by inspecting its state.
async fn docker_container_running(name: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect("/var/run/docker.sock").await else {
        return false;
    };
    let req = format!(
        "GET /containers/{name}/json HTTP/1.1\r\nHost: localhost\r\n\r\n"
    );
    let _ = stream.write_all(req.as_bytes()).await;
    let mut buf = vec![0u8; 8192];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    let body = String::from_utf8_lossy(&buf[..n]);
    body.contains("\"Running\":true")
}

/// POST /api/pipeline/gpu/pause — stop the llama container to free VRAM
///
/// Sets state to Stopping, spawns a background task to stop the container,
/// then transitions to Off. Returns immediately with the transitional state.
pub async fn pause_gpu(
    State(state): State<AppState>,
) -> axum::Json<serde_json::Value> {
    let current = state.metrics.gpu_state();
    if current != GpuState::On {
        info!(state = current.as_str(), "GPU pause requested but not in On state");
        return axum::Json(serde_json::json!({ "gpu_state": current.as_str() }));
    }

    state.metrics.set_gpu_state(GpuState::Stopping);
    info!("GPU stopping — sending docker stop to llama container");

    let metrics = state.metrics.clone();
    tokio::spawn(async move {
        match docker_post("/containers/llama/stop?t=5").await {
            Ok(status) if status == 204 || status == 304 => {
                info!(status, "llama container stopped successfully");
                metrics.set_gpu_state(GpuState::Off);
            }
            Ok(status) => {
                warn!(status, "Unexpected status stopping llama container");
                metrics.set_gpu_state(GpuState::Off);
            }
            Err(e) => {
                warn!(error = %e, "Failed to stop llama container");
                // Still mark as Off — the container may have stopped anyway,
                // and the user can retry resume.
                metrics.set_gpu_state(GpuState::Off);
            }
        }
    });

    axum::Json(serde_json::json!({ "gpu_state": "stopping" }))
}

/// POST /api/pipeline/gpu/resume — start the llama container and wait for health
///
/// Sets state to Starting, spawns a background task to start the container
/// and poll its health endpoint. Transitions to On when healthy.
pub async fn resume_gpu(
    State(state): State<AppState>,
) -> axum::Json<serde_json::Value> {
    let current = state.metrics.gpu_state();
    if current != GpuState::Off {
        info!(state = current.as_str(), "GPU resume requested but not in Off state");
        return axum::Json(serde_json::json!({ "gpu_state": current.as_str() }));
    }

    state.metrics.set_gpu_state(GpuState::Starting);
    info!("GPU starting — sending docker start to llama container");

    let metrics = state.metrics.clone();
    tokio::spawn(async move {
        match docker_post("/containers/llama/start").await {
            Ok(status) if status == 204 || status == 304 => {
                info!(status, "llama container start command accepted");
            }
            Ok(status) => {
                warn!(status, "Unexpected status starting llama container");
            }
            Err(e) => {
                warn!(error = %e, "Failed to start llama container");
                metrics.set_gpu_state(GpuState::Off);
                return;
            }
        }

        // Poll llama health endpoint until it reports healthy (up to 3 minutes)
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_secs(180);

        loop {
            if tokio::time::Instant::now() > deadline {
                warn!("Timed out waiting for llama container health");
                // Container started but model loading is slow — check if
                // the container itself is at least running.
                if docker_container_running("llama").await {
                    info!("llama container is running despite health timeout — marking On");
                    metrics.set_gpu_state(GpuState::On);
                } else {
                    warn!("llama container not running after timeout — marking Off");
                    metrics.set_gpu_state(GpuState::Off);
                }
                return;
            }

            tokio::time::sleep(std::time::Duration::from_secs(3)).await;

            match client.get("http://llama:8000/health").send().await {
                Ok(resp) if resp.status().is_success() => {
                    info!("llama container healthy — GPU is On");
                    metrics.set_gpu_state(GpuState::On);
                    return;
                }
                Ok(resp) => {
                    debug_health_poll(resp.status().as_u16());
                }
                Err(_) => {
                    // Container still starting up
                }
            }
        }
    });

    axum::Json(serde_json::json!({ "gpu_state": "starting" }))
}

fn debug_health_poll(status: u16) {
    tracing::debug!(status, "llama health poll — not yet healthy");
}
