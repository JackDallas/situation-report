# Situation Report — Data Source Reference

All source files are in `backend/crates/sources/src/`.

---

## Summary Table

| # | Source | ID | Auth | Type | Interval | Env Var(s) |
|---|--------|-----|------|------|----------|------------|
| 1 | ACLED | `acled` | OAuth2 password | Poll | 6h | `ACLED_EMAIL`, `ACLED_PASSWORD` |
| 2 | AirplanesLive | `airplaneslive` | None | Poll | 120s | — |
| 3 | adsb.lol | `adsb-lol` | None | Poll | 120s | — |
| 4 | adsb.fi | `adsb-fi` | None | Poll | 120s | — |
| 5 | AIS | `ais` | API key (WS) | Stream | — | `AISSTREAM_API_KEY` |
| 6 | BGP RIS Live | `bgp` | None | Stream | — | — |
| 7 | CertStream | `certstream` | None | Stream | — | — |
| 8 | Cloudflare Radar | `cloudflare` | Bearer token | Poll | 15m | `CLOUDFLARE_API_TOKEN` |
| 9 | Cloudflare BGP | `cloudflare-bgp` | Bearer token | Poll | 30m | `CLOUDFLARE_API_TOKEN` |
| 10 | FIRMS | `firms` | API key (URL) | Poll | 30m | `FIRMS_MAP_KEY` |
| 11 | GDELT Doc | `gdelt` | None | Poll | 15m | — |
| 12 | GDELT GEO 2.0 | `gdelt-geo` | None | Poll | 20m | — |
| 13 | GeoConfirmed | `geoconfirmed` | None | Poll | 60m | — |
| 14 | Global Fishing Watch | `gfw` | Bearer token | Poll | 30m | `GFW_API_TOKEN` |
| 15 | GPSJam | `gpsjam` | None | Poll | 6h | — |
| 16 | IODA | `ioda` | None | Poll | 10m | — |
| 17 | NOTAM / NATS UK | `notam` | None | Poll | 10m | — |
| 18 | Nuclear / Safecast | `nuclear` | None | Poll | 30m | — |
| 19 | OONI | `ooni` | None | Poll | 30m | — |
| 20 | OpenSky | `opensky` | OAuth2 client creds | Poll | 220s | `OPENSKY_CLIENT_ID`, `OPENSKY_CLIENT_SECRET` |
| 21 | OTX AlienVault | `otx` | API key (header) | Poll | 1h | `OTX_API_KEY` |
| 22 | RSS News (26 feeds) | `rss-news` | None | Poll | 5m | — |
| 23 | Shodan Stream | `shodan-stream` | API key (URL) | Stream | — | `SHODAN_API_KEY` |
| 24 | Shodan Discovery | `shodan-discovery` | API key (URL) | Poll | 24h | `SHODAN_API_KEY` |
| 25 | Shodan ICS Monitor | `shodan-search` | API key (URL) | Poll | 1h | `SHODAN_API_KEY` |
| 26 | Telegram | `telegram` | MTProto session | Stream | — | `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `TELEGRAM_SESSION_PATH` |
| 27 | USGS Seismic | `usgs` | None | Poll | 5m | — |

**Disabled:** ACLED (403 — requires Research tier institutional email)

---

## 1. ACLED (Armed Conflict Location & Event Data)

- **File:** `acled.rs`
- **Source type:** `acled`
- **Status:** DISABLED (requires institutional email for Research tier)
- **Type:** Polling | **Interval:** 6 hours
- **Endpoints:**
  - Token: `POST https://acleddata.com/oauth/token`
  - Data: `GET https://acleddata.com/api/acled/read`
- **Auth:** OAuth2 password grant → bearer token. Env: `ACLED_EMAIL`, `ACLED_PASSWORD`
- **Rate limits:** Page limit 5,000 records/page. 403 triggers re-auth.
- **Format:** JSON
- **Lookback:** 10 days
- **Country filter:** Iran, Israel, Ukraine, Russia, Yemen, Syria, Lebanon, Sudan, Myanmar
- **Event type:** `ConflictEvent` — fatalities >10 = Critical, >0 = High, else Medium
- **Notes:** Requires myACLED Research tier or institutional email. Personal tier only gets aggregated data.

---

## 2. AirplanesLive / ADS-B Aggregators

- **File:** `adsb.rs` (constructor: `airplaneslive()`), re-exported via `airplaneslive.rs`
- **Source types:** `airplaneslive`, `adsb-lol`, `adsb-fi`
- **Type:** Polling | **Interval:** 120 seconds
- **Endpoints:**
  - Military: `GET https://api.airplanes.live/v2/mil`
  - Point: `GET https://api.airplanes.live/v2/point/{lat}/{lon}/{radius}`
  - Squawk: `GET https://api.airplanes.live/v2/sqk/{code}`
  - Failover: `https://api.adsb.one/v2` (same paths)
  - adsb.lol: `https://api.adsb.lol/v2`
  - adsb.fi: `https://opendata.adsb.fi/api/v2` (geo uses v3)
- **Auth:** None
- **Rate limits:** Minimum 3,000ms gap between HTTP requests (self-enforced). 429 with Retry-After respected. Registry backoff suppressed — self-managed cooldown.
- **Format:** JSON (readsb format — `ac` array)
- **Per poll:** 1 military endpoint + 1 rotated regional point query (16 regions, 250nm radius) + squawk 7700 emergency query
- **Event type:** `FlightPosition` — ICAO hex, callsign, lat/lon, altitude, speed, track, squawk
- **Military detection:** dbFlags bit 0, callsign prefix matching (E-3, KC-135, B-52, F-35, etc.)
- **User-Agent:** `SituationReport/1.0 (military-aviation-monitor)`

---

## 3. AIS (Vessel Tracking)

- **File:** `ais.rs`
- **Source type:** `ais`
- **Type:** Streaming (WebSocket)
- **Endpoint:** `wss://stream.aisstream.io/v0/stream`
- **Auth:** API key in WebSocket subscription message. Env: `AISSTREAM_API_KEY`
- **Rate limits:** None — global bounding box subscription
- **Format:** JSON over WebSocket
- **Subscription:** `PositionReport` and `ShipStaticData` messages, global bbox `[[-90,-180],[90,180]]`
- **Event type:** `VesselPosition` — MMSI, ship name, lat/lon, SoG, CoG, heading, nav status
- **Military detection:** MMSI prefix matching (US Navy 338/369, Iran Navy 422)
- **Keepalive:** Ping frame every 30s, stats every 60s
- **Region classification:** 30 named maritime regions including chokepoints (Hormuz, Bab-el-Mandeb, Suez, Malacca, Taiwan Strait)

---

## 4. BGP (RIPE RIS Live)

- **File:** `bgp.rs`
- **Source type:** `bgp`
- **Type:** Streaming (WebSocket)
- **Endpoint:** `wss://ris-live.ripe.net/v1/ws/`
- **Auth:** None (public feed)
- **Rate limits:** None — subscribes to full BGP UPDATE stream
- **Format:** JSON over WebSocket
- **Subscribe:** `{"type":"ris_subscribe","data":{"type":"UPDATE","socketOptions":{"includeRaw":false}}}`
- **Monitored ASNs:** Iran (12880, 48159, 6736, 58224, 197207, 44244), Israel (378, 8551, 9116), Ukraine (6849, 15895), Russia (12389, 8402)
- **Event type:** `BgpAnomaly` — only route withdrawals for monitored ASNs are broadcast (announcements counted only — ~6000/min)
- **Dedup:** 5-minute window per (origin_asn, prefix) to suppress flapping
- **Stats:** Logged every 5 minutes

---

## 5. CertStream (Certificate Transparency)

- **File:** `certstream.rs`
- **Source type:** `certstream`
- **Type:** Streaming (WebSocket)
- **Endpoint:** `wss://certstream.calidog.io`
- **Auth:** None (public CT log firehose)
- **Rate limits:** None — filtered client-side
- **Format:** JSON over WebSocket
- **Domain filters:** `.gov.ir`, `.mil.ir`, `.gov.il`, `.mil.il`, `.gov.ua`, `.mil.ua`, `.mod.gov.`, `.irgc.ir`, `.gov.ru`, `.mil.ru`
- **Event type:** `CertIssued` — primary domain, SANs, issuer org, cert index
- **Reconnect:** Exponential backoff 1s → 60s max

---

## 6. Cloudflare Radar

- **File:** `cloudflare.rs`
- **Source types:** `cloudflare` (outages/anomalies), `cloudflare-bgp` (BGP leaks)
- **Type:** Polling | **Intervals:** 15m (outages), 30m (BGP leaks)
- **Auth:** Bearer token. Env: `CLOUDFLARE_API_TOKEN`

### Outages + Traffic Anomalies (`cloudflare`)
- **Endpoints:**
  - `GET https://api.cloudflare.com/client/v4/radar/annotations/outages?dateRange=7d&location={cc}`
  - `GET https://api.cloudflare.com/client/v4/radar/traffic_anomalies?location={cc}&dateRange=7d`
- **Countries (rotated 2/poll):** IR, IL, UA, RU, YE, SY, SD, MM, BH, QA, KW, AE
- **Rate limits:** 429 propagated. 250ms delay between countries.
- **Event type:** `InternetOutage` — outages = High, anomalies = Medium

### BGP Leaks (`cloudflare-bgp`)
- **Endpoint:** `GET https://api.cloudflare.com/client/v4/radar/bgp/leaks/events?per_page=25&sort_by=time&sort_order=desc`
- **Event type:** `BgpLeak` — origin ASN, leak ASN, prefix count

---

## 7. FIRMS (NASA Fire Data)

- **File:** `firms.rs`
- **Source type:** `firms`
- **Type:** Polling | **Interval:** 30 minutes
- **Endpoint:** `GET https://firms.modaps.eosdis.nasa.gov/api/area/csv/{MAP_KEY}/VIIRS_SNPP_NRT/{bbox}/1`
- **Auth:** API key in URL path. Env: `FIRMS_MAP_KEY`
- **Rate limits:** Rotates through 10 bounding boxes (1 per poll) to stay within per-query data caps
- **Format:** CSV
- **Regions (10):** middle_east, eastern_europe, north_africa, sub_saharan_africa, south_asia, east_asia, southeast_asia, south_america, north_america, oceania
- **Event type:** `ThermalAnomaly` — FRP (fire radiative power), satellite, confidence
- **Severity:** FRP >100 = High, >50 = Medium, else Low
- **Dedup:** In-memory HashSet of (lat×10000, lon×10000, acq_date, acq_time), 50K cap
- **Filter:** Low-confidence detections discarded

---

## 8. GDELT Doc API

- **File:** `gdelt.rs`
- **Source type:** `gdelt`
- **Type:** Polling | **Interval:** 15 minutes
- **Endpoint:** `GET https://api.gdeltproject.org/api/v2/doc/doc?query={q}&mode=ArtList&maxrecords=250&format=json`
- **Auth:** None (public API)
- **Rate limits:** 429 propagated. 15s timeout. 1 retry after 2s delay.
- **Format:** JSON
- **Rotating queries (7):** "iran israel war", "ukraine russia war", "yemen houthi", "sudan conflict", "strait hormuz", "cyber attack iran", "missile strike"
- **Event type:** `NewsArticle` — title, URL, tone, source country, domain, language
- **Severity:** tone < -5.0 = High, < -2.0 = Medium, else Low
- **Dedup:** `last_seen` watermark (seendate string, lexicographic)
- **Notes:** Coordinates are country centroids (approximate)

---

## 9. GDELT GEO 2.0

- **File:** `gdelt_geo.rs`
- **Source type:** `gdelt-geo`
- **Type:** Polling | **Interval:** 20 minutes
- **Endpoint:** `GET https://api.gdeltproject.org/api/v2/geo/geo?query={q}&format=GeoJSON&maxpoints=250`
- **Auth:** None
- **Rate limits:** 429 propagated. 15s timeout. 404 silently skipped (non-geographic terms).
- **Format:** GeoJSON
- **Rotating queries (7):** "iran", "ukraine", "yemen", "israel", "syria", "persian gulf", "sudan"
- **Event type:** `GeoNews` — exact lat/lon from features, name, URL, tone
- **Severity:** tone < -5.0 = High, < -2.0 = Medium, else Low

---

## 10. GeoConfirmed (OSINT Conflict Geolocations)

- **File:** `geoconfirmed.rs`
- **Source type:** `geoconfirmed`
- **Type:** Polling | **Interval:** 60 minutes
- **Endpoint:** `GET https://geoconfirmed.azurewebsites.net/api/Placemark/{conflict}/1/{pageSize}` (page 1, pageSize=50)
- **Auth:** None (Azure-hosted public API)
- **Rate limits:** 429 propagated. Only page 1 per conflict per poll.
- **Format:** JSON
- **Conflicts (7):** Ukraine, Israel, Syria, Yemen, DRC, Sahel, Myanmar
- **Event type:** `GeoEvent` — UUID dedup, icon-derived titles, equipment categories (tank, drone, missile)
- **Severity:** Destroyed = High, spotted = Medium
- **Dedup:** DB-level via ON CONFLICT on source_id (`geoconfirmed:{uuid}`)

---

## 11. Global Fishing Watch

- **File:** `gfw.rs`
- **Source type:** `gfw`
- **Type:** Polling | **Interval:** 30 minutes
- **Endpoint:** `POST https://gateway.api.globalfishingwatch.org/v3/events?limit=100&offset={n}&sort=+start`
- **Auth:** Bearer token. Env: `GFW_API_TOKEN`
- **Rate limits:** 429 propagated. Paginated (100/page).
- **Format:** JSON (POST body with GeoJSON polygon + date range)
- **Datasets (alternated):** `public-global-fishing-events:latest`, `public-global-loitering-events:latest`
- **Regions (8 rotated):** Middle East/Indian Ocean, East Asia/Pacific, Europe/Atlantic, West Africa, East Africa, Americas Pacific, Oceania, Arctic
- **Lookback:** 24 hours
- **Event type:** `FishingEvent` — vessel ID/name, lat/lon, region, maritime sub-region tags

---

## 12. GPSJam (GPS Interference)

- **File:** `gpsjam.rs`
- **Source type:** `gpsjam`
- **Type:** Polling | **Interval:** 6 hours
- **Endpoints (tried in order):**
  - `GET https://gpsjam.org/api/data/{YYYY-MM-DD}`
  - `GET https://gpsjam.org/data/{YYYY-MM-DD}.json`
  - `GET https://gpsjam.org/api/v1/interference?date={YYYY-MM-DD}`
- **Auth:** None
- **Rate limits:** Tries next URL on failure. Returns empty if all fail.
- **Format:** JSON (H3 hex grid)
- **Threshold:** Only cells >=10% jamming reported
- **Event type:** `GpsInterference` — H3 index, lat/lon, jamming percentage
- **Severity:** >50% = Critical, >30% = High, >15% = Medium, else Low
- **Lookback:** Previous day's data
- **Notes:** API not officially documented — best-effort scraping

---

## 13. IODA (Internet Outage Detection)

- **File:** `ioda.rs`
- **Source type:** `ioda`
- **Type:** Polling | **Interval:** 10 minutes
- **Endpoint:** `GET https://api.ioda.inetintel.cc.gatech.edu/v2/outages/alerts?from={unix}&until={unix}&entityType=country&entityCode={cc}`
- **Auth:** None (Georgia Tech public API)
- **Rate limits:** 429 propagated. Lookback 20 minutes.
- **Format:** JSON
- **Countries (rotated 1/poll):** IR, IL, UA, RU, YE, SD
- **Signal types:** `bgp`, `ping-slash24`, `merit-nt`
- **Filter:** Only `warning` and `critical` alerts (skip `normal`)
- **Event type:** `InternetOutage` — signal type, ratio, level
- **Severity:** `critical` → High, others → Medium

---

## 14. NOTAM (NATS UK PIB)

- **File:** `notam.rs`
- **Source type:** `notam`
- **Type:** Polling | **Interval:** 10 minutes (internal hourly re-fetch: `NATS_POLL_INTERVAL_SECS = 3600`)
- **Endpoint:** `GET https://pibs.nats.co.uk/operational/pibs/PIB.xml`
- **Auth:** None
- **Rate limits:** Internal hourly throttle
- **Format:** XML (ICAO EAD-format PIB)
- **Q-code filter:** QRALC, QRTCA, QFAHC, QFALC + conflict keyword detection
- **Critical FIRs:** OIIX (Tehran), ORBB (Baghdad), OBBB (Bahrain)
- **Event type:** `AirspaceEvent` — NOTAM text, Q-code, FIR
- **Severity:** QR/QW in critical FIRs = Critical, QR/QW elsewhere = High, else Medium
- **Notes:** No lat/lon — NOTAMs reference FIRs not coordinates. FAA API planned but not implemented.

---

## 15. Nuclear / Radiation (Safecast)

- **File:** `nuclear.rs`
- **Source type:** `nuclear`
- **Type:** Polling | **Interval:** 30 minutes
- **Endpoint:** `GET https://api.safecast.org/measurements.json?distance={km}&latitude={lat}&longitude={lon}&captured_after={ISO}`
- **Auth:** None (public Safecast API)
- **Rate limits:** 429 propagated. Lookback 24 hours.
- **Format:** JSON
- **Regions (3):** Iran (32N,51E,1000km), Turkey (39N,35E,1000km), Gulf (26N,50E,1000km)
- **Thresholds:** Baseline = 50 CPM, Alert = 100 CPM
- **Dedup:** In-memory measurement ID HashSet
- **Event type:** `RadiationAlert` — CPM value, device ID, lat/lon, height

---

## 16. OONI (Network Interference)

- **File:** `ooni.rs`
- **Source type:** `ooni`
- **Type:** Polling | **Interval:** 30 minutes
- **Endpoint:** `GET https://api.ooni.io/api/v1/measurements?probe_cc={cc}&test_name=web_connectivity&since={ISO}&limit=100`
- **Auth:** None (public API)
- **Rate limits:** 429 propagated. Lookback 2 hours.
- **Format:** JSON
- **Countries (rotated 1/poll):** IR, RU, UA, MM, SD, SY, YE
- **Filter:** Only anomalous or confirmed-blocked measurements emitted
- **Event type:** `CensorshipEvent` — measurement UID, probe country, test name, input URL
- **Severity:** `confirmed` = High, `anomaly` = Medium

---

## 17. OpenSky (Flight Tracking)

- **File:** `opensky.rs`
- **Source type:** `opensky`
- **Type:** Polling | **Interval:** 220 seconds (~10 regions × 3927 polls/day ≈ within 4000 credit limit)
- **Endpoints:**
  - Token: `POST https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token`
  - States: `GET https://opensky-network.org/api/states/all?lamin={}&lomin={}&lamax={}&lomax={}`
- **Auth:** OAuth2 client credentials → bearer token (optional, falls back to unauthenticated). Env: `OPENSKY_CLIENT_ID`, `OPENSKY_CLIENT_SECRET`
- **Rate limits:** 4,000 API credits/day (authenticated). Each bbox poll = 1 credit.
- **Format:** JSON (state vectors — 17-element arrays per aircraft)
- **Regions (10 rotated):** middle_east, ukraine, persian_gulf, red_sea_yemen, western_europe, east_asia, southeast_asia, north_africa, south_america, north_america
- **Military callsigns:** REACH, RCH, DUKE, FORTE, JAKE, VIPER, EVAC, TOPPS, NCHO, NATO, RRR, HOMER, LAGR, ETHYL, SENTRY, GORDO, IAF
- **Event type:** `FlightPosition` — ICAO24, callsign, lat/lon, altitude, velocity, heading, squawk

---

## 18. OTX (AlienVault Open Threat Exchange)

- **File:** `otx.rs`
- **Source type:** `otx`
- **Type:** Polling | **Interval:** 1 hour
- **Endpoints:**
  - Subscribed: `GET https://otx.alienvault.com/api/v1/pulses/subscribed?limit=50&modified_since={ISO}`
  - Search: `GET https://otx.alienvault.com/api/v1/search/pulses?q={query}&limit=20`
- **Auth:** API key in header. Env: `OTX_API_KEY`, Header: `X-OTX-API-KEY`
- **Rate limits:** 429 propagated. Lookback 2 hours for subscribed.
- **Format:** JSON
- **Search queries (5 rotated):** "iran apt", "israel cyber", "ukraine apt", "sandworm", "charming kitten"
- **Event type:** `ThreatIntel` — pulse name, adversary, tags, indicator count
- **Severity:** Has adversary = High, else Medium

---

## 19. RSS News Feeds

- **File:** `rss_news.rs`
- **Source type:** `rss-news`
- **Type:** Polling | **Interval:** 5 minutes (3 feeds per cycle, rotating through all 26)
- **Auth:** None
- **Rate limits:** Browser-like User-Agent to avoid 403s
- **User-Agent:** `Mozilla/5.0 (compatible; SituationReport/1.0; +https://github.com)`
- **Format:** RSS/Atom XML

### Feeds (26)

| ID | Feed | Source |
|----|------|--------|
| bbc-world | `https://feeds.bbci.co.uk/news/world/rss.xml` | BBC |
| guardian-world | `https://www.theguardian.com/world/rss` | Guardian |
| france24 | `https://www.france24.com/en/rss` | France 24 |
| dw | `https://rss.dw.com/rdf/rss-en-all` | Deutsche Welle |
| aljazeera-en | `https://www.aljazeera.com/xml/rss/all.xml` | Al Jazeera |
| middleeasteye | `https://www.middleeasteye.net/rss` | Middle East Eye |
| timesofisrael | `https://www.timesofisrael.com/feed/` | Times of Israel |
| meduza | `https://meduza.io/rss/en/all` | Meduza |
| moscowtimes | `https://www.themoscowtimes.com/rss/news` | Moscow Times |
| scmp | `https://www.scmp.com/rss/91/feed` | SCMP |
| yonhap | `https://en.yna.co.kr/RSS/news.xml` | Yonhap |
| allafrica | `https://allafrica.com/tools/headlines/rdf/latest/headlines.rdf` | AllAfrica |
| warontherocks | `https://warontherocks.com/feed/` | War on the Rocks |
| breakingdefense | `https://breakingdefense.com/feed/` | Breaking Defense |
| usni | `https://news.usni.org/feed` | USNI News |
| bellingcat | `https://www.bellingcat.com/feed/` | Bellingcat |
| crisisgroup | `https://www.crisisgroup.org/rss.xml` | Crisis Group |
| hackernews | `https://feeds.feedburner.com/TheHackersNews` | The Hacker News |
| bleepingcomputer | `https://www.bleepingcomputer.com/feed/` | Bleeping Computer |
| krebsonsecurity | `https://krebsonsecurity.com/feed/` | Krebs on Security |
| therecord | `https://therecord.media/feed/` | The Record |
| reliefweb | `https://reliefweb.int/updates/rss.xml` | ReliefWeb |
| un-news | `https://news.un.org/feed/subscribe/en/news/all/rss.xml` | UN News |
| world-nuclear-news | `https://world-nuclear-news.org/rss` | World Nuclear News |
| armscontrol | `https://www.armscontrol.org/rss.xml` | Arms Control Assoc |
| gcaptain | `https://gcaptain.com/feed/` | gCaptain |

- **Event type:** `NewsArticle`
- **Dedup:** Two-buffer rolling window (5K/10K GUIDs)

---

## 20. Shodan (3 sources)

**Env:** `SHODAN_API_KEY` (shared)
**API base:** `https://api.shodan.io`

### 20a. Shodan Stream (Alert Feed)
- **Source type:** `shodan-stream`
- **Type:** Streaming (NDJSON HTTP stream)
- **Endpoint:** `GET https://stream.shodan.io/shodan/alert?key={key}`
- **Reconnect:** Exponential backoff 1s → 60s
- **Event type:** `ShodanBanner` — IP, port, org, ASN, country, vulns, lat/lon
- **Severity:** Has CVEs = Critical, ICS port = High, else Info
- **ICS ports:** 502 (Modbus), 102 (S7), 2404 (IEC-104), 4840 (OPC-UA), 20000 (DNP3), 44818 (EtherNet/IP), 47808 (BACnet), and more

### 20b. Shodan Discovery (Auto-Monitor)
- **Source type:** `shodan-discovery`
- **Type:** Polling | **Interval:** 24 hours
- **Endpoints:** `/api-info`, `/shodan/host/search`, `/shodan/alert` (CRUD), `/shodan/alert/{id}/trigger/{triggers}`
- **Discovery:** 40+ port-based ICS queries across Iran, Gulf states, Israel, Iraq/Syria/Lebanon, maritime
- **Credit budget:** 1,000 credits/run, skips if <10

### 20c. Shodan ICS Monitor (Count-based)
- **Source type:** `shodan-search`
- **Type:** Polling | **Interval:** 1 hour
- **Endpoint:** `GET /shodan/host/count?key={key}&query={query}&facets=country`
- **Purpose:** Count of ICS-exposed hosts with country facets — trend monitoring

---

## 21. Telegram (OSINT Channels)

- **File:** `telegram.rs`
- **Source type:** `telegram`
- **Status:** Active (silently disabled if credentials missing)
- **Type:** Streaming (MTProto via `grammers` library)
- **Auth:** Telegram app credentials + SQLite session. Env: `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `TELEGRAM_SESSION_PATH` (default: `data/telegram.session`)
- **Rate limits:** Telegram's MTProto limits

### Channels (15, 4 tiers)

| Tier | Channel | Topic |
|------|---------|-------|
| 1 | CumtaAlertsEnglishChannel | Israel red alerts (middle-east) |
| 2 | noel_reports | Ukraine conflict |
| 2 | warmonitors | Multi-conflict |
| 2 | intelslava | Russia/Ukraine |
| 2 | sitreports | Situation reports |
| 2 | ClashReport | Multi-conflict |
| 3 | DeepStateEN | Ukraine analysis |
| 3 | rybar_in_english | Russia perspective |
| 3 | CIT_en | Conflict Intel Team |
| 3 | GeoConfirmed | OSINT geolocations |
| 3 | Intelsky | Aviation intel |
| 4 | DIUkraine | Ukraine defense intel |
| 4 | Ansarallah_MC | Houthi/Yemen |
| 4 | englishabuali | Abu Ali Express (Israel) |

- **Event type:** `TelegramMessage` — channel, message ID, text (500 char truncation)
- **Severity:** Keyword-based — "strike"/"missile"/"nuclear"/"BREAKING" = Critical; "military"/"airstrike" = High; else Medium
- **Notes:** Requires pre-authenticated session file

---

## 22. USGS Seismic

- **File:** `usgs.rs`
- **Source type:** `usgs`
- **Type:** Polling | **Interval:** 5 minutes
- **Endpoint:** `GET https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_hour.geojson`
- **Auth:** None (public GeoJSON feed)
- **Rate limits:** None documented
- **Format:** GeoJSON
- **Event type:** `SeismicEvent` — magnitude, place, depth, lat/lon, PAGER alert, tsunami flag, felt reports
- **Dedup:** In-memory HashSet of USGS event IDs
- **Special:** `is_potential_explosion()` — flags surface events (depth=0), shallow+small non-earthquakes
- **Region classification:** 12 bounding box regions; unmatched → "global"
