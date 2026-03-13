use axum::extract::State;
use axum::Json;
use serde::Serialize;

use crate::error::ApiError;
use crate::state::AppState;

#[derive(Clone, Debug, Serialize)]
pub struct SatelliteTle {
    pub name: String,
    pub norad_id: u32,
    pub tle_line1: String,
    pub tle_line2: String,
}

/// GET /api/satellite-tles — returns cached TLE data for FIRMS satellites.
pub async fn get_satellite_tles(
    State(state): State<AppState>,
) -> Result<Json<Vec<SatelliteTle>>, ApiError> {
    let tles = state
        .satellite_tles
        .read()
        .map_err(|e| anyhow::anyhow!("Failed to read satellite TLEs: {e}"))?
        .clone();
    Ok(Json(tles))
}

/// FIRMS satellite NORAD catalog IDs.
const FIRMS_SATELLITES: &[(u32, &str)] = &[
    (37849, "SUOMI NPP"),
    (43013, "NOAA-20 (JPSS-1)"),
    (54234, "NOAA-21 (JPSS-2)"),
];

/// Fetch TLEs from CelesTrak for the 3 FIRMS satellites.
pub async fn fetch_tles(client: &reqwest::Client) -> Vec<SatelliteTle> {
    let mut tles = Vec::new();

    for &(norad_id, name) in FIRMS_SATELLITES {
        let url = format!(
            "https://celestrak.org/NORAD/elements/gp.php?CATNR={norad_id}&FORMAT=TLE"
        );
        match client.get(&url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    tracing::warn!(
                        norad_id,
                        status = %resp.status(),
                        "CelesTrak TLE fetch failed"
                    );
                    continue;
                }
                match resp.text().await {
                    Ok(body) => {
                        let lines: Vec<&str> = body.trim().lines().collect();
                        if lines.len() >= 3 {
                            // CelesTrak TLE format: line 0 = name, line 1 = TLE line 1, line 2 = TLE line 2
                            tles.push(SatelliteTle {
                                name: name.to_string(),
                                norad_id,
                                tle_line1: lines[1].trim().to_string(),
                                tle_line2: lines[2].trim().to_string(),
                            });
                        } else if lines.len() == 2 {
                            // Some responses omit the name line
                            tles.push(SatelliteTle {
                                name: name.to_string(),
                                norad_id,
                                tle_line1: lines[0].trim().to_string(),
                                tle_line2: lines[1].trim().to_string(),
                            });
                        } else {
                            tracing::warn!(
                                norad_id,
                                lines = lines.len(),
                                "Unexpected TLE format from CelesTrak"
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(norad_id, error = %e, "Failed to read CelesTrak response body");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(norad_id, error = %e, "CelesTrak TLE request failed");
            }
        }
    }

    tles
}

/// Spawn a background task that refreshes satellite TLEs every 8 hours.
pub fn spawn_tle_refresh(
    satellite_tles: crate::state::SharedSatelliteTles,
    client: reqwest::Client,
) {
    tokio::spawn(async move {
        // Fetch immediately on startup
        let initial = fetch_tles(&client).await;
        if !initial.is_empty() {
            if let Ok(mut lock) = satellite_tles.write() {
                *lock = initial;
            }
            tracing::info!(
                count = FIRMS_SATELLITES.len(),
                "Satellite TLEs loaded from CelesTrak"
            );
        } else {
            tracing::warn!("Initial satellite TLE fetch returned no data");
        }

        // Refresh every 8 hours
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(8 * 3600));
        interval.tick().await; // consume first tick (already fetched above)

        loop {
            interval.tick().await;
            let updated = fetch_tles(&client).await;
            if !updated.is_empty() {
                if let Ok(mut lock) = satellite_tles.write() {
                    *lock = updated;
                }
                tracing::info!("Satellite TLEs refreshed from CelesTrak");
            } else {
                tracing::warn!("Satellite TLE refresh returned no data — keeping cached data");
            }
        }
    });
}
