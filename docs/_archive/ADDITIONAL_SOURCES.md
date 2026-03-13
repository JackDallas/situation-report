# Additional OSINT sources — research findings

> **Purpose:** Supplementary reference for the warfare monitoring dashboard. Covers source categories discovered through further research that aren't fully documented in SOURCES.md or FLIGHT-TRACKING.md. Each section explains what the source provides, how to access it, what it costs, and its intelligence value for the current Iran–Israel conflict.

---

## 1. ACARS messaging — Airframes.io

**The single most underrated OSINT source for military aviation.**

ADS-B tells you *where* an aircraft is. ACARS tells you *why* it's there. ACARS (Aircraft Communications Addressing and Reporting System) is a digital messaging system that aircraft use to communicate with ground stations, airlines, and ATC. Messages include position reports, ETAs, fuel status, maintenance requests, weather data, and operational instructions. Military aircraft use ACARS variants too.

### Sources

| Source | URL | Access | Notes |
|--------|-----|--------|-------|
| Airframes.io | https://app.airframes.io | Free (web search) | 4+ billion messages archived. Search by callsign, registration, ICAO hex, message content. The "real deal" for ACARS aggregation. |
| tbg.airframes.io | https://tbg.airframes.io | Free (web) | Military-focused ACARS map. Syncs ADS-B positions with ACARS messages in real-time. Scrappy but bleeding-edge. Run by thebaldgeek. |
| thebaldgeek GitHub | https://thebaldgeek.github.io | Free | Comprehensive guide to setting up your own ACARS/VDL/HFDL receivers with RTL-SDR hardware. |

### Message types that matter

- **ADS-C (Automatic Dependent Surveillance - Contract):** Position reports sent over ACARS, often from aircraft over oceans or in areas without ADS-B coverage. Catches aircraft that ADS-B misses entirely.
- **CPDLC (Controller-Pilot Data Link):** Text-based ATC communications. Shows routing instructions, altitude changes, clearances.
- **OOOI reports:** Out (gate departure), Off (wheels up), On (wheels down), In (gate arrival). Automated flight logging.
- **HFDL (High Frequency Data Link):** Long-range data link over HF radio. Catches aircraft anywhere in the world, including mid-ocean and polar routes. Uses ground stations in places like Al Muharraq (Bahrain) — directly relevant to Gulf monitoring.
- **SATCOM:** Satellite-based ACARS via Inmarsat/Iridium. Global coverage, no ground station needed. This is how MH370 was tracked after ADS-B was lost.

### How to feed

If you have an RTL-SDR dongle (~£25) and antenna, you can decode ACARS/VDL on VHF frequencies 129–136 MHz. Software: `acarsdec` (ACARS), `dumpvdl2` (VDL2), `dumphfdl` (HFDL). Feed to airframes.io via their client. Docker setup available: https://github.com/sdr-enthusiasts/docker-acarshub

### Intelligence value

During the Feb 28 strikes, military tankers and ISR aircraft were visible on ADS-B, but their ACARS messages would reveal fuel states, routing intentions, and operational context that positions alone don't provide. HFDL ground stations in the Gulf region capture messages from aircraft over the Persian Gulf and Arabian Sea even when ADS-B coverage is thin.

---

## 2. Maritime AIS — free alternatives to MarineTraffic

MarineTraffic is the best-known vessel tracker but is commercial. For the dashboard we need free, programmable AIS access. The Strait of Hormuz closure and Houthi Red Sea attacks make maritime monitoring critical.

### Free AIS sources

| Source | URL | Access | Rate limit | Notes |
|--------|-----|--------|------------|-------|
| **aisstream.io** | https://aisstream.io | Free account, WebSocket API | Not documented (generous) | **Best option.** Real-time WebSocket stream of global AIS data. Filter by bounding box, ship type, MMSI. JSON format. Includes vessel name, position, course, speed, destination, navigational status, and ship-to-ship messages. Also tracks SAR aircraft. No feeding required. |
| **AISHub** | https://www.aishub.net | Free — **requires feeding AIS data** | Unlimited for feeders | Data exchange co-op. You feed AIS data from your receiver, they give you access to the global aggregated feed via JSON/XML/CSV API. Need an AIS receiver and antenna (VHF 161.975/162.025 MHz). |
| **Global Fishing Watch** | https://globalfishingwatch.org | Free account, API available | Rate limited | Focused on fishing vessels but tracks all AIS-broadcasting ships. Good for detecting fishing fleet movements near conflict zones (Iran uses fishing vessels as proxy force). Map viewer + downloadable datasets. |

### What to watch for

- **AIS gaps in the Strait of Hormuz:** If tankers stop broadcasting in the strait, they're either being spoofed, jammed, or have turned off transponders. Compare AIS density with historical baselines.
- **Dark ships:** Vessels that go AIS-dark near sanctioned ports (Bandar Abbas, Hodeidah) are likely doing illicit transfers.
- **Iranian-flagged vessels:** MMSI prefix 422 = Iran. Track departures from Bandar Abbas, Bushehr, Chabahar.
- **Warship movements:** Naval vessels often broadcast AIS in non-combat situations. US 5th Fleet vessels (MMSI prefix 338/369) near Bahrain, Royal Navy near Oman.
- **SAR activity:** aisstream.io tracks search-and-rescue aircraft over water. Spikes in SAR activity = maritime incident.

### aisstream.io WebSocket example

```javascript
const ws = new WebSocket("wss://stream.aisstream.io/v0/stream");
ws.onopen = () => {
  ws.send(JSON.stringify({
    APIKey: "YOUR_KEY",
    BoundingBoxes: [
      [[25.5, 54.0], [27.0, 57.0]],  // Strait of Hormuz
      [[12.0, 43.0], [13.5, 44.0]]   // Bab-el-Mandeb
    ]
  }));
};
ws.onmessage = (event) => {
  const data = JSON.parse(event.data);
  // data.MetaData.ShipName, data.MetaData.MMSI, etc.
};
```

---

## 3. Seismic detection — USGS Earthquake API

Large explosions (MOAB-class bombs, bunker busters, ammunition depot detonations) register on seismographs. During the 2020 Beirut explosion, seismometers hundreds of km away picked up the blast as a ~3.3 magnitude event. The USGS API provides near-real-time earthquake data globally, and doesn't distinguish between earthquakes and explosions — both show up.

### API

- **URL:** https://earthquake.usgs.gov/fdsnws/event/1/
- **Format:** GeoJSON, XML, CSV
- **Auth:** None required
- **Rate limit:** Use GeoJSON feeds for continuous monitoring, API for custom queries
- **Latency:** Events typically appear within 5–20 minutes of occurrence

### Key endpoints

| Endpoint | Use |
|----------|-----|
| Real-time GeoJSON feed | `https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_hour.geojson` — all events in the last hour |
| Custom query | `https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&starttime=2026-02-28&minlatitude=25&maxlatitude=40&minlongitude=44&maxlongitude=64` — Iran bounding box |
| Significant events | `https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/significant_month.geojson` |

### What to look for

- **Shallow events (depth < 5km) in non-seismic zones:** An M2.0 event at 0km depth near Isfahan is almost certainly an explosion, not an earthquake. Iran's major fault lines run along the Zagros mountains — events far from these are suspicious.
- **Cluster patterns:** Multiple shallow events in rapid succession = bombardment pattern.
- **Event type field:** USGS sometimes classifies events as "explosion" or "mining explosion" when confident. Check the `type` field in the GeoJSON response.
- **Cross-reference with FIRMS:** A USGS seismic event + a FIRMS thermal anomaly at the same coordinates within minutes = confirmed strike with high confidence.

### IRIS / NSF SAGE

For raw seismogram data and more granular analysis: https://ds.iris.edu/ds/nodes/dmc/data/ — free, provides waveform data from the Global Seismographic Network. Useful for distinguishing explosion signatures (sharp onset, short duration) from earthquake signatures (gradual onset, longer coda).

---

## 4. GeoConfirmed — crowdsourced conflict geolocation

GeoConfirmed is a volunteer OSINT project that geolocates and verifies footage from conflict zones. They have dedicated maps for every conflict relevant to the dashboard, with verified lat/lon coordinates for each event.

### Maps

| Conflict | URL |
|----------|-----|
| Iran/Israel | https://geoconfirmed.org/iran |
| Israel/Gaza/Lebanon | https://geoconfirmed.org/israel |
| 7 October 2023 (dedicated) | https://geoconfirmed.org/7oct |
| Yemen | https://geoconfirmed.org/yemen |
| Syria | https://geoconfirmed.org/syria |
| Ukraine | https://geoconfirmed.org/ukraine |
| Myanmar | https://geoconfirmed.org/myanmar |
| Africa (terrorism) | https://geoconfirmed.org/africa |
| India-Pakistan | https://geoconfirmed.org/indpak |
| Pacific (China/Taiwan) | https://geoconfirmed.org/pacific |

### Data access

- **QGIS plugin:** https://github.com/Silverfish94/GeoConfirmed-QGIS — query and visualise GeoConfirmed data with filtering by date, keywords, factions, military units, and spatial criteria. Server-side filtering. Exports to GeoPackage for offline analysis.
- **Media search engine:** https://github.com/conflict-investigations/media-search-engine — cross-references URLs across GeoConfirmed, Bellingcat Civilian Harm, and Centre for Information Resilience (CIR) Eyes on Russia databases. Has a REST API.
- **ORBAT data:** GeoConfirmed now maintains order-of-battle data for Ukraine, Russia, Israel, Palestinian factions, and Myanmar at `https://geoconfirmed.org/orbat/{country}`.

### Intelligence value

GeoConfirmed's Iran/Israel map will be populating rapidly with geolocated strike footage. Each pin has verified coordinates, source media link, date, and faction tag. This is ground-truth data that can be cross-referenced with FIRMS thermal anomalies, USGS seismic events, and ADS-B/ACARS data to build a comprehensive strike picture.

---

## 5. Sentinel-1 SAR — radar satellite imagery

Synthetic Aperture Radar works day and night, through cloud cover, smoke, and dust — critical for a theatre where Iran has reportedly been setting oil fires for obscuration and where night strikes are the norm.

### Access

| Platform | URL | Notes |
|----------|-----|-------|
| Copernicus Data Space | https://dataspace.copernicus.eu | Free account. Browser + API access to all Sentinel data. New primary access point since Oct 2023. |
| Copernicus Browser | https://browser.dataspace.copernicus.eu | Visual search and download interface. Draw a box over Iran, filter by date, download GRD products. |
| NASA ASF DAAC | https://search.asf.alaska.edu | Free (NASA Earthdata login). Alternative access to Sentinel-1 data. Vertex search application. Python library `asf_search` for programmatic access. |
| Google Earth Engine | https://earthengine.google.com | Free for research. Pre-processed Sentinel-1 GRD data ready for analysis. Best for time-series change detection. |

### What it gives us

- **Sentinel-1C + 1D constellation:** 6-day revisit cycle restored. C-band SAR at 5–20m resolution, up to 400km swath.
- **Ship detection without AIS:** SAR can detect vessels that have turned off AIS transponders. Critical for monitoring dark ships in the Gulf and Red Sea. Sentinel-1C (launched Dec 2024) has an onboard AIS antenna for correlation.
- **Damage assessment:** Research published in late 2024 demonstrated automated building damage mapping in Ukraine using Sentinel-1 time series with machine learning. Same technique applies to Iran/Israel strikes. Compare pre-strike and post-strike backscatter to identify destroyed structures.
- **Oil spill detection:** Oil slicks are dark features on SAR. Detects tanker attacks, pipeline damage, deliberate oil fires.
- **Infrastructure monitoring:** Changes at military bases, airfields, port facilities visible in SAR even when optical satellites are blacked out by cloud/smoke.

### Latency

Sentinel-1 Near Real-Time products are available within ~1 hour of acquisition. Routine products within 3–24 hours. For time-critical monitoring, check the Copernicus Data Space for NRT products over the Middle East.

---

## 6. HF radio monitoring — military communications

Military forces use HF (shortwave) radio for long-range communications that can't be easily intercepted by local adversaries. During active operations, HF traffic increases measurably. You don't need to decrypt the content — traffic analysis (volume, timing, frequency usage) is intelligence in itself.

### Online receivers (no hardware needed)

| Platform | URL | Notes |
|----------|-----|-------|
| **KiwiSDR network** | http://kiwisdr.com/public/ | 600+ software-defined radio receivers worldwide accessible via web browser. 0–30 MHz coverage. Free. |
| **WebSDR** | http://www.websdr.org | University of Twente (Netherlands) is the flagship site. Covers 0–30 MHz with excellent sensitivity. |
| **SDR receiver map** | https://rx.skywavelinux.com | Map of all online SDR receivers (KiwiSDR, WebSDR, OpenWebRX). Find receivers geographically close to the Middle East. |
| **HFGCS quick-tune** | https://skywavelinux.com/hfgcs-quick-tune-list.html | Pre-tuned links to SDR receivers on USAF HFGCS frequencies. One-click monitoring of Emergency Action Messages. |

### Key frequencies

| Frequency (kHz) | Service | Intelligence value |
|------------------|---------|-------------------|
| 4724, 8992, 11175, 15016 | USAF HFGCS (primary) | Emergency Action Messages (EAMs), SKYKING broadcasts. Encrypted but traffic volume/timing indicates alert levels. Surge in EAMs = elevated nuclear/strategic posture. |
| 6697, 8776, 11244, 12082, 16309 | USAF HFGCS (secondary) | Used during exercises and real-world operations. Activity on secondary freqs = expanded operations. |
| 4625 | UVB-76 "The Buzzer" (Russia) | Continuous drone tone with occasional voice messages. Changes in pattern associated with Russian military alerts. |
| 5748.6–24883.6 (10 freqs) | US State Department E&E Network | Embassy emergency & evacuation networks. Activity = diplomatic crisis. Scanned via 2G ALE. |

### Priyom.org

- **URL:** https://priyom.org
- **Coverage:** Comprehensive database of military, diplomatic, and intelligence HF stations worldwide. Includes schedules, frequencies, callsigns, and transmission logs for HFGCS, Russian military nets, numbers stations, and diplomatic services.
- **HFGCS page:** https://priyom.org/military-stations/united-states/hfgcs
- **Russia military:** https://priyom.org/military-stations/russia
- **Station schedule:** https://priyom.org/number-stations/station-schedule

### What to monitor during the Iran war

1. **HFGCS EAM volume:** If the USAF is sending 2x or 3x the normal rate of Emergency Action Messages, strategic assets are being tasked. E-6B TACAMO aircraft (callsign HOMER*) relay these messages to nuclear submarines.
2. **HFGCS secondary frequency activation:** Activity on 6697/8776/11244 outside normal exercise windows = real-world operations.
3. **Russian military HF activity:** Russia has interests in the region (Syria, Iran relations). Increased Russian Navy HF traffic from Mediterranean bases = fleet movements.
4. **2G ALE soundings:** Automated Link Establishment soundings from new or unusual callsigns on military frequencies indicate new stations coming online — possibly forward-deployed communications.

---

## 7. Bellingcat & CIR investigation databases

### Bellingcat

- **Civilian Harm in Ukraine:** https://ukraine.bellingcat.com — verified incidents with coordinates
- **Geolocation Toolkit:** https://bellingcat.gitbook.io/toolkit — curated list of OSINT tools for geolocation, chronolocation, image/video verification
- **Auto-Archiver:** Open source tool for automatically archiving social media content before deletion

### Centre for Information Resilience (CIR)

- **Eyes on Russia:** Interactive conflict map with 11,600+ verified entries contributed by CIR, GeoConfirmed, Bellingcat, and independent volunteers
- **Built by C4ADS** — filterable by date, category, subcategory, location, keyword

### conflict-investigations/media-search-engine

- **GitHub:** https://github.com/conflict-investigations/media-search-engine
- **What it does:** Takes a social media URL and checks if that media has already been geolocated in Bellingcat, CIR, or GeoConfirmed databases. API and CLI available.
- **Use case:** When a video surfaces on Telegram claiming to show a strike in Iran, check if it's already been verified and geolocated by the OSINT community before treating it as new intelligence.

---

## 8. Additional satellite imagery sources

Beyond FIRMS (thermal) and Sentinel-1 (SAR), there are other free satellite sources:

| Source | URL | What it provides |
|--------|-----|------------------|
| Sentinel-2 | https://dataspace.copernicus.eu | Optical imagery at 10m resolution, 5-day revisit. Cloud-dependent but excellent for damage BDA when skies are clear. RGB + near-infrared bands. |
| Landsat 8/9 | https://earthexplorer.usgs.gov | 30m optical imagery, 16-day revisit. Free via USGS EarthExplorer. Longer history for baseline comparison. |
| Planet NICFI | https://www.planet.com/nicfi/ | Tropical forest monitoring — 4.77m resolution basemaps. Limited to tropical areas but covers parts of Yemen. |
| NASA Worldview | https://worldview.earthdata.nasa.gov | Browser-based visualisation of MODIS, VIIRS, and other NASA imagery. Near-real-time. Good for smoke plumes, large fires, and oil spills at regional scale. |
| Google Earth Timelapse | https://earthengine.google.com/timelapse/ | Decades of satellite imagery for tracking long-term changes at military/nuclear facilities. |

---

## 9. Prediction markets

Not a traditional OSINT source, but prediction markets aggregate crowd intelligence on conflict outcomes in real-time.

| Platform | URL | Notes |
|----------|-----|-------|
| Polymarket | https://polymarket.com | Crypto-based. Has markets on Iran/Israel conflict outcomes, oil prices, Hormuz closure duration. Liquid and responsive. |
| Metaculus | https://www.metaculus.com | Calibrated forecasting platform. Has structured questions on Iran nuclear program, conflict escalation, etc. |

### Why they matter

Prediction market prices move *fast* when new information surfaces, often faster than news cycles. A sudden price spike in "Will the US strike Iranian nuclear facilities by April 2026?" at 2am is a signal that something leaked or was observed before mainstream media caught it. IranMonitor.org already integrates prediction market data into its dashboard.

---

## Summary of new sources by priority

### Tier 1 — Integrate into dashboard immediately

| Source | Category | Cost | Why |
|--------|----------|------|-----|
| aisstream.io | Maritime AIS | Free | WebSocket API, real-time vessel tracking at Hormuz and Bab-el-Mandeb. The strait is *closed*. |
| USGS Earthquake API | Seismic | Free | Detect large explosions. Cross-reference with FIRMS for confirmed strikes. |
| GeoConfirmed Iran map | Geolocation | Free | Ground-truth verified strike locations from the OSINT community. |

### Tier 2 — High value, integrate soon

| Source | Category | Cost | Why |
|--------|----------|------|-----|
| Airframes.io | ACARS messaging | Free (web) | "ADS-B is the where, ACARS is the why." Military aircraft operational context. |
| Sentinel-1 SAR | Satellite imagery | Free | Night/cloud-penetrating damage assessment. Ship detection without AIS. |
| KiwiSDR + HFGCS | HF radio | Free | Strategic alert monitoring. EAM volume = nuclear/strategic posture. |

### Tier 3 — Supplementary / manual monitoring

| Source | Category | Cost | Why |
|--------|----------|------|-----|
| Priyom.org | HF radio database | Free | Reference for military station schedules and frequencies. |
| Bellingcat/CIR databases | Verification | Free | Cross-reference media and prevent misinformation entering the dashboard. |
| media-search-engine | Verification | Free | Automated check of social media against verified geolocation databases. |
| Sentinel-2 / Landsat | Optical satellite | Free | Clear-sky damage assessment at higher resolution than SAR. |
| Polymarket | Prediction markets | Free | Crowd intelligence on conflict trajectory. |
