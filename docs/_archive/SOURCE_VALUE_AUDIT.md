# Source Value Audit: Signal vs. Noise Analysis

**Date:** 2026-03-02
**Scope:** All 27 registered data sources in the Situation Report OSINT monitoring dashboard
**Purpose:** Categorize sources by intelligence value, identify noise generators, and recommend a filtering/routing strategy to surface actionable intelligence

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Source Tier Categorization](#source-tier-categorization)
3. [Per-Source Assessments](#per-source-assessments)
4. [Deep Dive: BGP](#deep-dive-bgp)
5. [Deep Dive: CertStream](#deep-dive-certstream)
6. [Recommended Pipeline Architecture](#recommended-pipeline-architecture)
7. [Concrete Code Changes](#concrete-code-changes)

---

## Executive Summary

The Situation Report ingests data from 27 sources across 7 domains (conflict, cyber, aviation, maritime, satellite, nuclear, news/OSINT). The system's core problem is that **position-tracking and infrastructure-monitoring sources generate 95%+ of event volume while contributing less than 5% of actionable intelligence**. BGP alone produced 32,463 events in 5 minutes during testing. Meanwhile, the sources that actually detect conflict events (ACLED, GeoConfirmed, GDELT-geo, FIRMS) and their corroborating signals (USGS seismic, IODA outages, NOTAM restrictions) are relatively low-volume and high-value.

The pipeline already handles this partially: `HIGH_VOLUME_TYPES` absorbs `flight_position`, `vessel_position`, `cert_issued`, and `shodan_banner` into summary buckets rather than emitting them individually on SSE. The BGP source was recently refactored to only broadcast withdrawals (not announcements). But several gaps remain:

- **GDELT-geo** emits `geo_news` (not in the `is_important` passthrough list), so geolocated news events are silently dropped from SSE
- **FIRMS thermal anomalies** (`thermal_anomaly`) are not in the passthrough list despite being a key input to the `confirmed_strike` and `conflict_thermal` correlation rules
- **Several sources are broken in production** (ACLED 403, AirplanesLive 429, NOTAM 401, GFW 0 events) and need fixes before their value can be assessed
- **CertStream** provides marginal value for conflict monitoring and should be reconsidered
- **RSS News** (25 feeds) and **GDELT Doc** (7 queries) are the primary news ingestion paths, creating potential duplication

### Key Numbers

| Metric | Value |
|--------|-------|
| Total registered sources | 27 |
| Streaming (WebSocket) sources | 3 (BGP, CertStream, AIS) |
| Polling sources | 24 |
| Currently broken/degraded | 5 (ACLED, AirplanesLive, NOTAM, GFW, OTX partial) |
| HIGH_VOLUME_TYPES absorbed | 4 (flight_position, vessel_position, cert_issued, shodan_banner) |
| Correlation rules | 8 |
| Event types used by rules | 12 unique types across all rules |

---

## Source Tier Categorization

### Tier 1: Core Intelligence (Direct conflict/incident detection)

These sources directly detect conflict events, strikes, military activity, or critical infrastructure events. Loss of any Tier 1 source creates a blind spot.

| Source | Event Type | Why Tier 1 |
|--------|-----------|------------|
| **ACLED** | `conflict_event` | Gold-standard conflict data with actors, fatalities, geolocations. Only source with verified fatality counts. |
| **GeoConfirmed** | `geo_event` | OSINT-verified geolocated military equipment sightings and strikes. High confidence. |
| **GDELT GEO 2.0** | `geo_news` | Geolocated news events with lat/lon. Primary source of geolocated breaking news. |
| **USGS Seismic** | `seismic_event` | Detects potential explosions via shallow seismic events. Critical for strike confirmation. |
| **FIRMS (NASA)** | `thermal_anomaly` | Satellite-detected fires/thermal hotspots. Confirms strikes, detects infrastructure attacks. |
| **IODA** | `internet_outage` | Country-level internet outage detection. Precursor/indicator of military operations. |
| **GPS Jam** | `gps_interference` | GPS interference detection. Strong indicator of electronic warfare / military operations. |

### Tier 2: Corroborating Intelligence (Confirms/contextualizes Tier 1)

These sources provide signals that confirm or add context to Tier 1 events. They feed correlation rules and situation clustering but rarely stand alone.

| Source | Event Type | Why Tier 2 |
|--------|-----------|------------|
| **Cloudflare Radar** | `internet_outage` | Outage and traffic anomaly detection. Corroborates IODA. |
| **Cloudflare BGP** | `bgp_leak` | BGP leak detection. Indicates infrastructure attacks or state censorship. |
| **BGP RIS Live** | `bgp_anomaly` (withdrawals only) | Route withdrawals for monitored ASNs. Feeds `coordinated_shutdown` and `infra_attack` rules. |
| **OONI** | `censorship_event` | Censorship measurements. Confirms coordinated shutdowns. |
| **OpenSky** | `flight_position` | Secondary aviation tracking. Cross-references AirplanesLive military flights. |
| **AirplanesLive** | `flight_position` | Military aircraft tracking (tankers, ISR, bombers). Feeds `military_strike` and `gps_military` rules. |
| **AIS** | `vessel_position` | Naval vessel tracking in chokepoints (Hormuz, Bab-el-Mandeb, Black Sea). Feeds `maritime_enforcement` rule. |
| **NOTAM** | `notam_event` | Airspace restrictions. Strong indicator of military operations. Feeds `military_strike` and `gps_military` rules. |
| **Telegram** | `telegram_message` | Real-time OSINT from monitored channels. Often first source to report events. |
| **Nuclear (Safecast)** | `nuclear_event` | Radiation monitoring. Critical for nuclear escalation scenarios. |

### Tier 3: Background Intelligence (Useful context, higher noise)

These sources provide useful intelligence but at a lower signal-to-noise ratio. They contribute to the overall intelligence picture but generate noise if not filtered.

| Source | Event Type | Why Tier 3 |
|--------|-----------|------------|
| **GDELT Doc** | `news_article` | News article discovery. Overlaps significantly with RSS News. Useful for non-English sources. |
| **RSS News** | `news_article` | 25 curated feeds covering defense, conflict, cyber, regional news. Primary English-language news. |
| **OTX (AlienVault)** | `threat_intel` | APT/threat intelligence pulses. Relevant but low update frequency. Feeds `apt_staging` rule. |
| **Shodan Discovery** | `shodan_banner` | ICS/SCADA device discovery. Situational awareness for infrastructure vulnerability. |
| **Shodan Search** | `shodan_banner` | Targeted ICS searches. Feeds `infra_attack` rule. |
| **GFW** | `fishing_event` | Fishing and loitering events. Low actionability for conflict monitoring. |

### Tier 4: Reconsider (Noise outweighs value)

These sources generate disproportionate noise relative to their intelligence contribution, or are unreliable in production.

| Source | Event Type | Why Tier 4 |
|--------|-----------|------------|
| **CertStream** | `cert_issued` | TLS certificate monitoring. See [deep dive](#deep-dive-certstream). Marginal conflict intelligence. |
| **Shodan Stream** | `shodan_banner` | Real-time banner stream. Very high volume, mostly routine. Better served by periodic search. |

---

## Per-Source Assessments

### 1. ACLED (Armed Conflict Location & Event Data)

- **Source ID:** `acled`
- **Event type:** `conflict_event`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 6 hours
- **Volume:** Moderate bursts. ~5,000 records per page, covers 10-day lookback across 9 countries.
- **Signal-to-noise:** Very high (>90%). Pre-filtered to conflict countries. Severity based on fatalities.
- **Correlation rules:** `confirmed_strike` (trigger), `conflict_thermal` (trigger)
- **Pipeline routing:** Passes `is_important` when fatalities > 0 (severity high/critical). Medium severity events (0 fatalities, e.g. protests) are silently correlated but not individually emitted on SSE.
- **Current problems:** 403 auth error. OAuth2 token or credentials may be expired/revoked.
- **Recommendation:** **FIX AUTH URGENTLY.** This is the single most important conflict data source. Verify ACLED_EMAIL and ACLED_PASSWORD credentials. The source has OAuth2 with refresh token support, so the 403 may indicate a revoked token. Consider adding a health check alert for this source.

### 2. GeoConfirmed

- **Source ID:** `geoconfirmed`
- **Event type:** `geo_event`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 30 minutes
- **Volume:** Low-moderate. Up to 500 placemarks per conflict, 7 conflicts monitored (Ukraine, Israel, Syria, Yemen, DRC, Sahel, Myanmar).
- **Signal-to-noise:** Very high (>95%). Data is crowdsourced-then-verified geolocated military events. Equipment categories derived from icon codes.
- **Correlation rules:** None directly (feeds SituationGraph for clustering).
- **Pipeline routing:** Passes `is_important` as `geo_event`.
- **Current problems:** None reported. Azure API appears stable.
- **Recommendation:** **Keep as-is.** This is a uniquely valuable source. Consider increasing poll frequency to 15 minutes for faster detection of new verifications.

### 3. GDELT GEO 2.0

- **Source ID:** `gdelt-geo`
- **Event type:** `geo_news`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 20 minutes
- **Volume:** High. Up to 250 geolocated points per query, 7 queries rotated. In production, this source generated 504 situations.
- **Signal-to-noise:** Moderate (~40-60%). Geolocated news is inherently valuable but includes routine reporting alongside breaking events. Tone-based severity helps prioritize.
- **Correlation rules:** None directly (feeds SituationGraph).
- **Pipeline routing:** **BUG: `geo_news` is NOT in the `is_important` passthrough list.** Events are correlated in the window but never emitted on SSE as individual events. They do enter the SituationGraph. This is likely unintentional -- `geo_event` (GeoConfirmed) passes through but `geo_news` does not.
- **Current problems:** The `geo_news` event type is likely a naming oversight -- it should either be added to `is_important` or renamed to `geo_event`.
- **Recommendation:** **Add `geo_news` to the `is_important` passthrough list.** This is probably the highest-impact single fix. 504 situations were generated from this source but none of its events reached SSE individually.

### 4. GDELT Doc API

- **Source ID:** `gdelt`
- **Event type:** `news_article`
- **Tier:** 3 (Background)
- **Poll interval:** 15 minutes
- **Volume:** Moderate. Up to 250 articles per query, 7 queries rotated. Deduplication via seendate watermark.
- **Signal-to-noise:** Low-moderate (~20-30%). Articles are keyword-matched but include tangential results. Significant overlap with RSS News.
- **Correlation rules:** None (news articles feed enrichment pipeline, not correlation rules).
- **Pipeline routing:** Passes `is_important` as `news_article`. Gets Haiku enrichment (translation, summarization, entity extraction).
- **Current problems:** Occasionally returns empty or non-JSON responses.
- **Recommendation:** **Reduce scope to complement RSS.** GDELT's value is non-English source discovery and quantitative tone analysis. Consider reducing to 3-4 queries focused on conflict-specific terms that RSS feeds are less likely to cover (e.g., "missile strike", "drone attack", "Hormuz"). Remove queries that overlap with RSS coverage (e.g., "iran israel war" duplicates what BBC/Al Jazeera/TOI already cover).

### 5. USGS Seismic

- **Source ID:** `usgs`
- **Event type:** `seismic_event`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 5 minutes
- **Volume:** Very low. Only events in monitored bounding boxes (Iran, Israel/Lebanon/Syria, Ukraine, Red Sea/Yemen). Typically 0-5 per hour.
- **Signal-to-noise:** High (>80%). Filtered to monitored regions. Explosion heuristic (depth=0, shallow+small, explicit explosion label) is well-designed.
- **Correlation rules:** `military_strike` (trigger). Cross-references with flight positions and NOTAMs.
- **Pipeline routing:** Passes `is_important` when magnitude >= 4.0. Potential explosions get `critical` severity and always pass through.
- **Current problems:** None.
- **Recommendation:** **Keep as-is.** This source is perfectly tuned -- low volume, high value, critical for strike detection.

### 6. NASA FIRMS

- **Source ID:** `firms`
- **Event type:** `thermal_anomaly`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 30 minutes
- **Volume:** Moderate. Varies by region and season. Low-confidence detections are pre-filtered.
- **Signal-to-noise:** Moderate (~50%). Thermal anomalies include natural wildfires alongside strikes/explosions, but the FRP (Fire Radiative Power) metric helps distinguish. In conflict zones, correlation with ACLED events dramatically increases actionability.
- **Correlation rules:** `confirmed_strike` (trigger), `conflict_thermal` (trigger). Cross-references with ACLED conflict events.
- **Pipeline routing:** **BUG: `thermal_anomaly` is NOT in the `is_important` passthrough list.** These events enter the correlation window and can trigger the `confirmed_strike` and `conflict_thermal` rules, but individual high-FRP thermal anomalies never reach SSE on their own.
- **Current problems:** Only the routing gap above.
- **Recommendation:** **Add `thermal_anomaly` to `is_important` with a threshold (e.g., FRP > 50 or severity high/critical).** In conflict zones, a large thermal anomaly is immediately actionable. The correlation rules handle the cross-referencing, but operators should also see individual hot spots.

### 7. IODA (Internet Outage Detection & Analysis)

- **Source ID:** `ioda`
- **Event type:** `internet_outage`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 10 minutes
- **Volume:** Very low. Only emits non-normal alerts (warning/critical) for 6 monitored countries. Typically 0-3 per poll cycle.
- **Signal-to-noise:** Very high (>90%). Pre-filtered to anomalous conditions. Normal-level alerts are skipped.
- **Correlation rules:** Feeds `coordinated_shutdown` rule (alongside BGP and censorship events).
- **Pipeline routing:** Passes `is_important` as `internet_outage`.
- **Current problems:** None reported.
- **Recommendation:** **Keep as-is.** Excellent signal quality. Consider adding more countries (Lebanon, Myanmar) to the monitor list.

### 8. GPSJam

- **Source ID:** `gpsjam`
- **Event type:** `gps_interference`
- **Tier:** 1 (Core Intelligence)
- **Poll interval:** 6 hours
- **Volume:** Very low (one daily snapshot). Filters to cells with >10% interference.
- **Signal-to-noise:** High when data is available. GPS interference is a strong indicator of military electronic warfare, especially around conflict zones.
- **Correlation rules:** `gps_military` (trigger). Cross-references with flight positions and NOTAMs.
- **Pipeline routing:** Passes `is_important` as `gps_interference`.
- **Current problems:** The API endpoint is not well-documented and the source tries 3 different URL patterns. May not currently be returning data.
- **Recommendation:** **Keep but verify data availability.** If the API endpoints are not working, consider scraping the gpsjam.org tile data or using an alternative GPS interference data source. The intelligence value is very high when it works.

### 9. Cloudflare Radar

- **Source ID:** `cloudflare`
- **Event type:** `internet_outage`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 15 minutes
- **Volume:** Low. Rotates through 2 countries per cycle from a list of 12. Checks both outage annotations and traffic anomalies. 7-day lookback window.
- **Signal-to-noise:** High (~80%). Only emits actual outage annotations and traffic anomalies.
- **Correlation rules:** Feeds `coordinated_shutdown` and `infra_attack` rules via `internet_outage` event type.
- **Current problems:** Requires CLOUDFLARE_API_TOKEN.
- **Recommendation:** **Keep as-is.** Good corroborating source for IODA. The 7-day lookback may produce duplicates -- consider adding a seen-set for deduplication similar to USGS.

### 10. Cloudflare BGP Leak Monitor

- **Source ID:** `cloudflare-bgp`
- **Event type:** `bgp_leak`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 30 minutes
- **Volume:** Very low. Fetches latest 25 BGP leak events.
- **Signal-to-noise:** High (~85%). BGP leaks are inherently significant. Curated by Cloudflare's detection engine.
- **Pipeline routing:** Passes `is_important` as `bgp_leak`.
- **Current problems:** Requires CLOUDFLARE_API_TOKEN.
- **Recommendation:** **Keep as-is.** Much better value than raw BGP RIS Live for detecting intentional route manipulation.

### 11. BGP RIS Live

- **Source ID:** `bgp`
- **Event type:** `bgp_anomaly`
- **Tier:** 2 (Corroborating)
- **See [deep dive](#deep-dive-bgp) for full analysis.**
- **Poll interval:** Streaming (WebSocket)
- **Volume:** After recent refactor, only withdrawals from monitored ASNs are broadcast. Previously 32,463 events/5min; now much lower.
- **Signal-to-noise:** Moderate (~30-50% for withdrawals). Individual withdrawals are routine; burst patterns are meaningful.
- **Current problems:** Volume management after refactor appears adequate but untested under sustained load.
- **Recommendation:** **Add burst detection.** See deep dive.

### 12. OONI (Censorship)

- **Source ID:** `ooni`
- **Event type:** `censorship_event`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 30 minutes
- **Volume:** Moderate. Up to 100 measurements per country, rotated through 7 countries. Only anomalous/confirmed measurements emitted.
- **Signal-to-noise:** Moderate (~40-60%). Censorship anomalies are common in monitored countries (Iran, Russia) but confirmed blocks are highly actionable.
- **Correlation rules:** Feeds `coordinated_shutdown` rule.
- **Pipeline routing:** Passes `is_important` as `censorship_event`.
- **Current problems:** None reported.
- **Recommendation:** **Keep as-is.** Consider weighting confirmed blocks (severity "high") more heavily than anomalies (severity "medium"). The `coordinated_shutdown` rule should ideally require at least one confirmed block, not just anomalies.

### 13. AirplanesLive

- **Source ID:** `airplaneslive`
- **Event type:** `flight_position`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 45 seconds
- **Volume:** High. Fetches all military aircraft globally, plus one rotated regional query, plus squawk 7700 emergencies. Potentially hundreds of aircraft per poll.
- **Signal-to-noise:** Moderate for raw positions (~15% actionable), high for military-tagged and emergency aircraft (~70%). Good filtering: military callsign prefixes, high-value platform types, dbFlags bit detection.
- **Correlation rules:** Feeds `military_strike` and `gps_military` rules via `flight_position`.
- **Pipeline routing:** Absorbed into `HIGH_VOLUME_TYPES` (30s summary bucket). Military, high-value, and emergency flights pass through `is_routine_high_volume` filter to SituationGraph.
- **Current problems:** **Permanent 429 rate limiting.** The API blocks requests with HTTP 429. The 1500ms delay between sub-queries is insufficient.
- **Recommendation:** **Increase poll interval to 90-120 seconds.** Remove the regional point query when rate-limited -- prioritize /mil and /squawk/7700 endpoints. Consider implementing adaptive interval that backs off on 429s. Also consider whether OpenSky can serve as the primary aviation source with AirplanesLive as fallback.

### 14. OpenSky

- **Source ID:** `opensky`
- **Event type:** `flight_position`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 90 seconds
- **Volume:** High. Returns all aircraft in one of 4 bounding boxes per poll. Potentially 100-500 aircraft per query.
- **Signal-to-noise:** Low for raw positions (~5-10% military). Military callsign detection is less sophisticated than AirplanesLive (no dbFlags, no type codes).
- **Correlation rules:** Same as AirplanesLive (feeds `military_strike` and `gps_military` via `flight_position`).
- **Pipeline routing:** Absorbed into `HIGH_VOLUME_TYPES`. Military flights pass through `is_routine_high_volume` to SituationGraph.
- **Current problems:** Rate limits with OAuth2 credit system (4000 credits/day). Currently at the edge of the budget.
- **Recommendation:** **Keep as secondary source.** OpenSky is the backup for AirplanesLive. Consider reducing bounding boxes to 2 highest-priority regions and polling less frequently (3 minutes). The primary value is cross-referencing AirplanesLive military detections.

### 15. AIS (Vessel Tracking)

- **Source ID:** `ais`
- **Event type:** `vessel_position`
- **Tier:** 2 (Corroborating)
- **Poll interval:** Streaming (WebSocket)
- **Volume:** High. Continuous stream from 6 maritime chokepoints. Volume depends on traffic density.
- **Signal-to-noise:** Low for raw positions (~5%). Military MMSI detection is limited to 3 prefix patterns. Most traffic is commercial.
- **Correlation rules:** Feeds `maritime_enforcement` rule (alongside `fishing_event`).
- **Pipeline routing:** Absorbed into `HIGH_VOLUME_TYPES` (30s summary bucket). Military and anomalous vessels (dark-ship, violation) pass through `is_routine_high_volume` to SituationGraph.
- **Current problems:** **WebSocket unstable, disconnects every 2 minutes.** This is likely an API key or network issue with aisstream.io.
- **Recommendation:** **Fix WebSocket stability.** The AIS stream is valuable for maritime domain awareness but worthless if it disconnects constantly. Investigate whether the AISSTREAM_API_KEY is valid and whether the bounding box count (6) exceeds the API tier limits. Consider reducing to 3-4 highest-priority areas. Add reconnection backoff similar to CertStream.

### 16. NOTAM (Airspace Restrictions)

- **Source ID:** `notam`
- **Event type:** `notam_event`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 2 hours (or 4 hours for fallback providers)
- **Volume:** Low. Filtered to conflict-relevant FIR codes. Military/GPS NOTAMs prioritized via keyword filtering.
- **Signal-to-noise:** High (~70%). NOTAMs for airspace closures and GPS interference advisories in conflict zones are highly actionable.
- **Correlation rules:** Feeds `military_strike` and `gps_military` rules.
- **Pipeline routing:** Passes `is_important` as `notam_event`.
- **Current problems:** **401 from Autorouter API.** The source has a fallback to NATS UK XML feed and FAA NOTAM API, but the primary source is broken.
- **Recommendation:** **Verify Autorouter credentials or disable that provider.** The source already has 3 fallback providers (NATS UK, FAA, ICAO). If Autorouter auth cannot be fixed, just disable it and rely on fallbacks. The fallback has already been flagged by the `autorouter_failed` AtomicBool.

### 17. Telegram OSINT

- **Source ID:** `telegram`
- **Event type:** `telegram_message`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 60 seconds
- **Volume:** Depends on channel activity. Keyword-based severity classification.
- **Signal-to-noise:** Variable (~30-60%). Depends entirely on which channels the bot is subscribed to. OSINT channels like Intel Slava Z, War Monitor, OSINTTECHNICAL have high signal when curated.
- **Pipeline routing:** Passes `is_important` as `telegram_message`.
- **Current problems:** Requires TELEGRAM_BOT_TOKEN. The bot must be added to monitored channels.
- **Recommendation:** **Keep but curate channel list.** The value is entirely dependent on channel selection. Document the recommended channel list. Consider implementing per-channel severity weighting -- some channels are more reliable than others.

### 18. Nuclear / Radiation (Safecast)

- **Source ID:** `nuclear`
- **Event type:** `nuclear_event`
- **Tier:** 2 (Corroborating)
- **Poll interval:** 30 minutes
- **Volume:** Low. 3 monitored regions (Iran, Turkey, Gulf), 24h lookback. Safecast is citizen-science data, coverage is sparse.
- **Signal-to-noise:** Low in normal conditions (~5% elevated readings, mostly natural variation). Extremely high if an actual nuclear event occurs.
- **Pipeline routing:** Passes `is_important` as `nuclear_event`.
- **Current problems:** Safecast coverage in monitored regions is very sparse. May produce zero events for days.
- **Recommendation:** **Keep as-is.** The low volume means zero noise cost. The value in an escalation scenario is incalculable. Consider adding EURDEP or other radiation monitoring networks for better coverage.

### 19. OTX (AlienVault)

- **Source ID:** `otx`
- **Event type:** `threat_intel`
- **Tier:** 3 (Background)
- **Poll interval:** 1 hour
- **Volume:** Low. Fetches subscribed pulses + one rotated search query (5 queries total: "iran apt", "israel cyber", etc.).
- **Signal-to-noise:** Moderate (~40-50%). Pulses with named adversaries (severity "high") are more actionable. Some search results are tangential.
- **Correlation rules:** Feeds `apt_staging` rule (alongside `cert_issued`).
- **Pipeline routing:** Passes `is_important` as `threat_intel`.
- **Current problems:** **Parse failures on some queries.** May need error handling improvements.
- **Recommendation:** **Keep but fix parse errors.** The `apt_staging` correlation rule depends on threat intel data. Consider reducing to 3 focused queries and increasing subscribed pulse monitoring.

### 20. Shodan Stream (Alert Monitoring)

- **Source ID:** `shodan-stream`
- **Event type:** `shodan_banner`
- **Tier:** 4 (Reconsider)
- **Poll interval:** Streaming (WebSocket)
- **Volume:** High. Receives all banners matching Shodan alert filters. Volume depends on alert configuration.
- **Signal-to-noise:** Low (~5-10%). Most banners are routine scans. Requires well-tuned Shodan alerts to be useful.
- **Correlation rules:** Feeds `infra_attack` rule.
- **Pipeline routing:** Absorbed into `HIGH_VOLUME_TYPES` (60s summary bucket). Always classified as routine in `is_routine_high_volume`.
- **Current problems:** Requires Shodan API key and configured alerts.
- **Recommendation:** **Consider disabling in favor of periodic search.** The stream produces high volume with low actionability. Shodan Search (periodic polling) is more targeted and controllable. If kept, ensure alerts are narrowly scoped to ICS ports in monitored countries only.

### 21. Shodan Discovery

- **Source ID:** `shodan-discovery`
- **Event type:** `shodan_banner`
- **Tier:** 3 (Background)
- **Poll interval:** Variable (based on search configuration)
- **Volume:** Low-moderate. Periodic ICS device discovery.
- **Signal-to-noise:** Moderate (~30%). Finds exposed ICS devices but most are already known.
- **Current problems:** `tag:ics` queries fail on Shodan edu plan.
- **Recommendation:** **Keep but adjust queries for plan limitations.** Replace `tag:ics` with port-based queries (e.g., `port:502,2404,47808`). The ICS port list is already well-defined in the source.

### 22. Shodan Search

- **Source ID:** `shodan-search`
- **Event type:** `shodan_banner`
- **Tier:** 3 (Background)
- **Poll interval:** Variable
- **Volume:** Low. Targeted searches.
- **Signal-to-noise:** Higher than stream (~40%) because queries are targeted.
- **Correlation rules:** Feeds `infra_attack` rule.
- **Current problems:** Same edu plan limitations as Discovery.
- **Recommendation:** **Keep as the primary Shodan ingestion method.** Use targeted queries rather than stream.

### 23. GFW (Global Fishing Watch)

- **Source ID:** `gfw`
- **Event type:** `fishing_event`, `loitering_event`, `encounter_event`, `ais_gap`
- **Tier:** 3 (Background)
- **Poll interval:** 30 minutes
- **Volume:** Should be moderate but currently producing 0 events.
- **Signal-to-noise:** Low (~15%) for general fishing. Higher for loitering and AIS gap events in conflict areas.
- **Correlation rules:** Feeds `maritime_enforcement` rule.
- **Pipeline routing:** `fishing_event` passes `is_important`.
- **Current problems:** **Reports healthy but 0 events.** The API may be returning data outside the combined bounding box, or the response format may have changed.
- **Recommendation:** **Debug the zero-event issue.** Check if the GFW API v3 response format matches the parser. Log the raw response. If the data quality remains low, consider downgrading to polling every 2 hours.

### 24. RSS News Feeds

- **Source ID:** `rss-news`
- **Event type:** `news_article`
- **Tier:** 3 (Background) -- but collectively very valuable
- **Poll interval:** 5 minutes (3 feeds per cycle, 25 total feeds, full rotation every ~42 minutes)
- **Volume:** Moderate. 3 feeds per cycle, deduplication via GUID tracking.
- **Signal-to-noise:** Moderate (~30-40%). Curated feed list is excellent (BBC, Al Jazeera, Times of Israel, Bellingcat, etc.) but many articles are routine.
- **Pipeline routing:** Passes `is_important` as `news_article`. Gets Haiku enrichment.
- **Current problems:** None reported. Good diversity of sources.
- **Recommendation:** **Keep as-is but address duplication with GDELT.** Since both GDELT Doc and RSS News emit `news_article` events, the same story may be enriched twice by Haiku. Consider adding URL-based cross-source deduplication, or reducing GDELT Doc to queries that do not overlap with the RSS feed list. The 25-feed roster is well-curated for the mission.

### 25. CertStream

- **Source ID:** `certstream`
- **Event type:** `cert_issued`
- **Tier:** 4 (Reconsider)
- **See [deep dive](#deep-dive-certstream) for full analysis.**
- **Poll interval:** Streaming (WebSocket)
- **Volume:** High raw volume, heavily filtered. Only `.gov.xx` and `.mil.xx` domains for monitored countries pass through.
- **Signal-to-noise:** Very low (~1-5%) even after filtering.
- **Current problems:** Disconnects every 60 seconds from certstream.calidog.io.
- **Recommendation:** **Disable or make opt-in.** See deep dive.

---

## Deep Dive: BGP

### What BGP RIS Live Provides

The BGP source connects to RIPE's RIS Live WebSocket and receives real-time BGP UPDATE messages from route collectors worldwide. It monitors 13 specific ASNs across 4 countries:

- **Iran (6 ASNs):** AS12880, AS48159, AS6736, AS58224, AS197207, AS44244
- **Israel (3 ASNs):** AS378, AS8551, AS9116
- **Ukraine (2 ASNs):** AS6849, AS15895
- **Russia (2 ASNs):** AS12389, AS8402

### The Volume Problem (Now Addressed)

The original implementation broadcast ALL BGP messages (announcements + withdrawals) to the pipeline. At ~100 messages/second, this produced **32,463 events in 5 minutes** and was the single largest noise source in the system. The recent refactor correctly addressed this:

- **Announcements:** Now counted for observability but NOT broadcast or persisted individually. Only tracked as counters logged every 5 minutes.
- **Withdrawals:** Only withdrawals involving monitored ASNs are broadcast to the pipeline as `bgp_anomaly` events with severity "high".

### Are Withdrawals Correlated with Real-World Events?

**Yes, but with important caveats:**

1. **Individual withdrawals are common and routine.** A single prefix withdrawal from AS12880 (Iran's ITC) could mean anything from routine maintenance to a misconfiguration. These happen regularly.

2. **Burst patterns are highly significant.** When Iran shuts down internet access (as documented in 2019 protests, 2022 Mahsa Amini protests), the BGP signature is clear: dozens to hundreds of withdrawals from the same ASN within minutes. Similarly, targeted attacks on Ukrainian internet infrastructure show up as concentrated withdrawal bursts from AS6849/AS15895.

3. **The `coordinated_shutdown` correlation rule already handles this.** It triggers on `bgp_anomaly` events and cross-references with `internet_outage` (IODA/Cloudflare) and `censorship_event` (OONI). This is the right approach.

### What We Would Lose Without BGP Announcements

**Nothing operationally meaningful.** BGP announcements (route appearing/changing) are the internet's normal background activity. They indicate that routing is working, not that it is breaking. The only theoretical use case -- detecting BGP hijacking via anomalous announcements -- is better served by Cloudflare's BGP Leak Monitor, which has dedicated detection logic.

### Recommendation: Add Burst Detection

The current implementation broadcasts every individual withdrawal. For the `coordinated_shutdown` rule to work effectively, it should detect withdrawal bursts rather than individual events. Recommended approach:

1. **In the BGP source:** Accumulate withdrawals per ASN in a sliding 5-minute window. Only broadcast a `bgp_anomaly` event when the count exceeds a threshold (e.g., 10 withdrawals from the same ASN in 5 minutes).

2. **Alternatively, in the pipeline:** The correlation window already provides the aggregation. The `coordinated_shutdown` rule could check for N withdrawals from the same entity_id (AS number) within the window. However, doing it at the source level reduces pipeline load.

3. **Keep announcement counting** for the 5-minute stats log. This provides operational awareness without pipeline cost.

**Estimated volume reduction:** Individual withdrawals from monitored ASNs are perhaps 1-10/minute during normal operations. With burst detection (threshold of 10 in 5 minutes), this drops to 0 events during normal operations and only fires during actual outage events.

---

## Deep Dive: CertStream

### What CertStream Provides

CertStream connects to the Certificate Transparency (CT) log stream via `wss://certstream.calidog.io`. The raw stream is extremely high volume (thousands of certificates per second globally). The source filters to certificates containing domain patterns for monitored countries:

- `.gov.ir`, `.mil.ir`, `.irgc.ir` (Iran)
- `.gov.il`, `.mil.il` (Israel)
- `.gov.ua`, `.mil.ua` (Ukraine)
- `.mod.gov.` (military domains)
- `.gov.ru`, `.mil.ru` (Russia)

### Intelligence Value Assessment

**The intelligence value is marginal for conflict monitoring.** Here is what CertStream can theoretically detect:

1. **New government/military infrastructure coming online.** A new `.mil.ir` certificate could indicate infrastructure deployment. However, certificate issuance typically happens weeks/months before the infrastructure is operationally relevant.

2. **Phishing infrastructure setup.** Certificates for domains mimicking government sites could indicate APT staging. The `apt_staging` correlation rule uses `cert_issued` as a trigger for this purpose.

3. **Certificate renewal patterns.** Sudden changes in certificate issuance patterns for a country could indicate something, but this would require statistical baseline tracking that does not exist.

### Why the Value is Low

1. **No geographic correlation.** Certificates have no lat/lon. They cannot be placed on the map. In a system designed around geographic situational awareness, this is a fundamental limitation.

2. **Lag time is wrong.** Certificate issuance is a preparation activity, not an operational activity. By the time a certificate is issued, the event it might relate to is weeks away. For a real-time monitoring dashboard, this is not useful.

3. **The filtering produces near-zero results.** Government domains in these countries rarely issue certificates through public CT logs. Most use private CAs or certificate pinning. The CertStream filter will match very few certificates.

4. **Connection instability.** The `certstream.calidog.io` service disconnects every 60 seconds, meaning the source spends most of its time reconnecting rather than monitoring.

5. **The `apt_staging` rule is speculative.** Correlating certificate issuance with threat intel pulses to detect APT staging is theoretically sound but practically unreliable. The correlation window is too short (6h) for the timescales involved, and the signal-to-noise ratio makes false positives very likely.

### Recommendation

**Disable CertStream by default.** Make it opt-in via configuration for users who specifically want CT monitoring. The `apt_staging` rule should be updated to work with OTX threat intel data alone, or replaced with a rule that correlates Shodan banner discoveries with threat intel.

If CertStream is kept:
- Move from streaming to periodic batch querying via the [crt.sh API](https://crt.sh/) which provides searchable CT logs without WebSocket instability
- Query once per hour for new certificates matching monitored domains
- This would be far more reliable and consume essentially zero resources

---

## Recommended Pipeline Architecture

### Event Routing Categories

Based on this audit, events should be routed into 5 categories:

#### Category A: Always Pass Through (Individual SSE events)
These are immediately actionable and should always reach the frontend individually.

| Event Type | Source(s) | Condition |
|-----------|-----------|-----------|
| `conflict_event` | ACLED | fatalities > 0 |
| `geo_event` | GeoConfirmed | always |
| `geo_news` | GDELT GEO | always (CURRENTLY MISSING) |
| `seismic_event` | USGS | magnitude >= 4.0 OR potential_explosion |
| `nuclear_event` | Nuclear | always |
| `gps_interference` | GPSJam | always |
| `internet_outage` | IODA, Cloudflare | always |
| `bgp_leak` | Cloudflare BGP | always |
| `censorship_event` | OONI | confirmed only (anomalies to Category C) |
| `notam_event` | NOTAM | always |
| `telegram_message` | Telegram | always |
| `news_article` | GDELT, RSS | always (Haiku enrichment) |
| `threat_intel` | OTX | always |
| `thermal_anomaly` | FIRMS | FRP > 50 or severity high+ (CURRENTLY MISSING) |
| `source_health` | Internal | always |

#### Category B: Absorbed into Summaries (High-volume, low individual value)
These are high-volume position/status events that are summarized but not individually emitted.

| Event Type | Source(s) | Summary Interval |
|-----------|-----------|-----------------|
| `flight_position` | OpenSky, AirplanesLive | 30 seconds |
| `vessel_position` | AIS | 30 seconds |
| `shodan_banner` | Shodan Stream/Discovery/Search | 60 seconds |
| `cert_issued` | CertStream | 60 seconds |

These still enter the correlation window and can trigger rules. Military/anomalous variants bypass absorption via `is_routine_high_volume`.

#### Category C: Anomaly-Only (Only emit on burst/threshold detection)
These events should be accumulated and only emit when an anomaly pattern is detected.

| Event Type | Source(s) | Anomaly Trigger |
|-----------|-----------|----------------|
| `bgp_anomaly` | BGP RIS Live | 10+ withdrawals from same ASN in 5 minutes |
| `censorship_event` | OONI (anomaly-grade) | 5+ anomalies from same country in 30 minutes |
| `conflict_event` | ACLED | fatalities == 0, severity medium (protests etc.) |
| `thermal_anomaly` | FIRMS | severity low (FRP < 50), in non-conflict region |

#### Category D: Sampled (Reduce ingestion rate)
For sources where full ingestion is unnecessary.

| Source | Current Rate | Recommended Rate | Rationale |
|--------|-------------|-----------------|-----------|
| OpenSky | 90s (4 regions) | 180s (2 regions) | Backup source, reduce API credit usage |
| AirplanesLive | 45s | 90-120s | Rate limited at current speed |
| GDELT Doc | 15min, 7 queries | 20min, 4 queries | Overlaps with RSS |

#### Category E: Disabled by Default
These should be opt-in only.

| Source | Rationale |
|--------|-----------|
| CertStream | Marginal value, connection instability |
| Shodan Stream | Better served by periodic search |

### Correlation Rule Event Type Usage Map

This maps which rules consume which event types, showing the full dependency chain:

```
infra_attack:          shodan_banner + bgp_anomaly + internet_outage
military_strike:       flight_position + notam_event + seismic_event
confirmed_strike:      conflict_event + thermal_anomaly
coordinated_shutdown:  internet_outage + bgp_anomaly + censorship_event
maritime_enforcement:  vessel_position + fishing_event
apt_staging:           cert_issued + threat_intel
conflict_thermal:      thermal_anomaly + conflict_event
gps_military:          gps_interference + flight_position + notam_event
```

**Key observation:** Every correlation rule depends on at least one Tier 1 or Tier 2 source. The absorbed high-volume types (`flight_position`, `vessel_position`, `shodan_banner`, `cert_issued`) are all inputs to correlation rules, which is why they must remain in the correlation window even when not individually emitted on SSE.

---

## Concrete Code Changes

### Fix 1: Add `geo_news` to `is_important` passthrough (HIGH PRIORITY)

**File:** `backend/crates/pipeline/src/pipeline.rs`
**Location:** `is_important()` function, around line 133

Add `"geo_news"` to the always-pass editorial/low-volume types match arm:

```rust
// Always-pass editorial / low-volume types
"nuclear_event"
| "gps_interference"
| "news_article"
| "geo_event"
| "geo_news"          // <-- ADD THIS
| "censorship_event"
| "economic_event"
| "notam_event"
| "telegram_message"
| "threat_intel"
| "internet_outage"
| "fishing_event"
| "bgp_leak"
| "source_health" => true,
```

**Impact:** GDELT GEO events (the source that generated 504 situations) will now reach the SSE stream individually.

### Fix 2: Add `thermal_anomaly` to `is_important` with threshold (HIGH PRIORITY)

**File:** `backend/crates/pipeline/src/pipeline.rs`
**Location:** `is_important()` function

Add a conditional passthrough for significant thermal anomalies:

```rust
// Significant thermal anomalies (high FRP = potential strike/explosion)
"thermal_anomaly" => {
    let sev = event.severity.as_str();
    sev == "high" || sev == "critical"
},
```

**Impact:** High-FRP thermal hotspots in conflict zones will reach operators directly, not just via correlation rules.

### Fix 3: BGP burst detection at source level (MEDIUM PRIORITY)

**File:** `backend/crates/sources/src/bgp.rs`

Replace individual withdrawal broadcasting with burst detection:

```rust
// Add a per-ASN withdrawal counter
struct WithdrawalTracker {
    counts: HashMap<u32, (u64, Instant)>, // ASN -> (count, window_start)
    threshold: u64,
    window: Duration,
}

impl WithdrawalTracker {
    fn record(&mut self, asn: u32) -> bool {
        let entry = self.counts.entry(asn).or_insert((0, Instant::now()));
        if entry.1.elapsed() > self.window {
            *entry = (1, Instant::now());
            false
        } else {
            entry.0 += 1;
            entry.0 >= self.threshold
        }
    }
}
```

When the threshold is hit, emit a single `bgp_anomaly` event with the aggregated count in the payload. This replaces per-withdrawal broadcasting.

**Threshold recommendation:** 10 withdrawals from the same ASN in 5 minutes.

### Fix 4: Disable CertStream by default (LOW PRIORITY)

**File:** `backend/crates/server/src/main.rs`

Wrap CertStream registration in a feature flag:

```rust
// CertStream — opt-in, marginal value for conflict monitoring
if std::env::var("CERTSTREAM_ENABLED")
    .unwrap_or_else(|_| "false".to_string())
    .parse::<bool>()
    .unwrap_or(false)
{
    registry.register(Arc::new(sr_sources::certstream::CertstreamSource));
}
```

Add `CERTSTREAM_ENABLED=false` to `.env.example` with a comment explaining the rationale.

### Fix 5: Disable Shodan Stream by default (LOW PRIORITY)

**File:** `backend/crates/server/src/main.rs`

Same pattern as CertStream:

```rust
// Shodan Stream — opt-in, high volume. Use ShodanDiscovery + ShodanSearch instead.
if std::env::var("SHODAN_STREAM_ENABLED")
    .unwrap_or_else(|_| "false".to_string())
    .parse::<bool>()
    .unwrap_or(false)
{
    registry.register(Arc::new(sr_sources::shodan::ShodanStream::new()));
}
```

### Fix 6: Reduce GDELT Doc query overlap with RSS (LOW PRIORITY)

**File:** `backend/crates/sources/src/gdelt.rs`

Reduce QUERIES to conflict-specific terms not well covered by RSS:

```rust
const QUERIES: &[&str] = &[
    "missile strike",
    "drone attack",
    "cyber attack infrastructure",
    "strait hormuz blockade",
];
```

Remove broad terms like "iran israel war", "ukraine russia war" which are fully covered by the RSS feed list (BBC, Al Jazeera, Times of Israel, Meduza, etc.).

### Fix 7: AirplanesLive rate limit mitigation (MEDIUM PRIORITY)

**File:** `backend/crates/sources/src/airplaneslive.rs`

Increase `default_interval()` and add adaptive backoff:

```rust
fn default_interval(&self) -> Duration {
    Duration::from_secs(120) // Was 45s, 429s at that rate
}
```

In `poll()`, skip the regional point query if the military endpoint returns a 429. Prioritize `/mil` and `/squawk/7700` over regional queries.

### Summary of Fixes by Priority

| Priority | Fix | Impact |
|----------|-----|--------|
| **HIGH** | Add `geo_news` to `is_important` | Unblocks GDELT GEO events on SSE |
| **HIGH** | Add `thermal_anomaly` threshold to `is_important` | Surfaces strike indicators |
| **HIGH** | Fix ACLED authentication | Restores primary conflict data source |
| **MEDIUM** | BGP burst detection | Reduces noise, improves coordinated_shutdown rule |
| **MEDIUM** | AirplanesLive rate limit mitigation | Restores aviation tracking |
| **MEDIUM** | Fix AIS WebSocket stability | Restores maritime tracking |
| **MEDIUM** | Fix NOTAM auth (or rely on fallbacks) | Restores airspace restriction data |
| **LOW** | Disable CertStream by default | Reduces noise and resource usage |
| **LOW** | Disable Shodan Stream by default | Reduces noise |
| **LOW** | Reduce GDELT Doc query overlap | Reduces duplicate news enrichment |
| **LOW** | Debug GFW zero-event issue | Restores maritime fishing data |

---

*This audit was conducted by analyzing all 27 source implementations in `backend/crates/sources/src/`, the pipeline routing logic in `backend/crates/pipeline/src/pipeline.rs`, all 8 correlation rules in `backend/crates/pipeline/src/rules/`, and production health telemetry.*
