# Flight tracking sources — what we actually get and how to use them

> **Purpose:** Reference doc for Claude Code. Explains the three available flight data APIs, what each gives us, what each hides, rate limits, cost, and the recommended integration architecture for the warfare dashboard.

---

## The problem

Military and government aircraft are the primary intelligence targets for this dashboard. During the Feb 28 2026 US-Israeli strikes on Iran, OSINT analysts tracked KC-135 tanker racetrack patterns, B-2 stealth bomber movements, and RC-135 reconnaissance flights in real-time via open ADS-B data. The question is which API actually gives us access to that data, and at what cost.

There are three viable sources. Two are free. One costs $10/month. They have very different capabilities.

---

## Source 1: OpenSky Network (free)

- **URL:** https://opensky-network.org
- **API docs:** https://openskynetwork.github.io/opensky-api/rest.html
- **Auth:** OAuth2 client credentials (since March 2025). Create account → Account page → Create API Client → get `client_id` and `client_secret` in `credentials.json`
- **Cost:** Free
- **Rate limit:** 4,000 credits/day (registered), 400/day (anonymous). If you feed data to OpenSky, you get more.
- **Update frequency:** State vectors update every 5 seconds for authenticated users, 10 seconds anonymous.

### What it gives us

OpenSky is a non-profit research network. It collects raw ADS-B, Mode S, and FLARM data from ~6,000 volunteer sensors globally. It returns standard state vectors:

| Field | Description |
|-------|-------------|
| icao24 | 24-bit ICAO transponder address (hex) |
| callsign | Flight callsign (8 chars) |
| origin_country | Country of registration |
| longitude, latitude | WGS84 position |
| baro_altitude | Barometric altitude (metres) |
| velocity | Ground speed (m/s) |
| true_track | Track angle (degrees clockwise from north) |
| vertical_rate | Climb/descent rate (m/s) |
| on_ground | Boolean |
| squawk | Mode A transponder code |
| category | Aircraft category (0–20) |

Bounding box queries supported: `GET /api/states/all?lamin=25&lomin=44&lamax=40&lomax=63.5` (Iran example).

### What it does NOT give us

**OpenSky does not filter military aircraft.** This is a common misconception. Their FAQ explicitly states: "We do not block and filter aircraft as that would obviously be detrimental to the accuracy of the scientific data we support."

However, OpenSky has significant limitations for military tracking:

1. **No MLAT (multilateration).** OpenSky does not compute positions from multiple receivers for aircraft that broadcast Mode S but not ADS-B positions. Many military aircraft only broadcast Mode S (transponder on, but no GPS position). Without MLAT, you get the ICAO hex and altitude but no lat/lon. This is a major gap.

2. **Coverage gaps in the Middle East.** OpenSky's sensor network is densest in Europe and the US. Coverage over Iran, Yemen, and open water (Persian Gulf, Red Sea) is much thinner. Aircraft below sensor range or in gaps simply don't appear.

3. **Military aircraft turn transponders off.** OpenSky can't track what isn't broadcasting. Stealth aircraft, combat aircraft during active operations, and any aircraft wanting to be invisible will have transponders off entirely. No ADS-B source can help here.

4. **5-second minimum resolution.** Good enough for tracking tanker orbits, too slow for tactical flight paths.

5. **1-hour historical window only.** Cannot query data older than 1 hour via the REST API (historical data requires Trino shell access, granted to academic institutions).

### Verdict

**Use as the continuous background layer.** Poll all 4 Middle East bounding boxes every 30–60 seconds. At that rate, 4 regions × 1 request/minute × 1440 minutes/day = 5,760 credits/day — over the 4,000 limit. So either reduce to every 90 seconds (3,840/day, safe) or feed an ADS-B receiver to OpenSky for a higher quota. Good for detecting tanker orbits, ISR patterns, civilian airspace closures, and general air traffic disruption. Won't catch Mode S-only military aircraft without positions.

---

## Source 2: Airplanes.live (free) ← THE DISCOVERY

- **URL:** https://airplanes.live
- **API docs:** https://airplanes.live/api-guide/
- **Data fields:** https://airplanes.live/rest-api-adsb-data-field-descriptions/
- **Map:** https://globe.airplanes.live
- **Auth:** None required (currently)
- **Cost:** Free
- **Rate limit:** 1 request per second
- **Update frequency:** Real-time

### What it gives us

Airplanes.live is the largest independent community of **unfiltered** ADS-B/Mode S/**MLAT** feeders. This is the critical difference from OpenSky. They compute MLAT positions, they never filter or obfuscate results, and they have a dedicated military endpoint.

**Endpoints:**

| Endpoint | Description |
|----------|-------------|
| `/v2/point/{lat}/{lon}/{radius}` | All aircraft within radius (up to 250nm) of a point |
| `/v2/mil` | **All aircraft tagged as military** — single global query |
| `/v2/hex/{hex}` | Lookup by ICAO hex address |
| `/v2/callsign/{callsign}` | Lookup by callsign |
| `/v2/reg/{reg}` | Lookup by registration |
| `/v2/type/{type}` | All aircraft of a given ICAO type code (e.g. `C135`, `P8`, `E3`) |
| `/v2/squawk/{squawk}` | All aircraft squawking a specific code |
| `/v2/ladd` | All aircraft on the FAA's Limited Aircraft Data Displayed list |
| `/v2/pia` | All aircraft using Privacy ICAO Addresses |

**Example:** `curl https://api.airplanes.live/v2/mil` — returns every military aircraft currently broadcasting, worldwide, with positions. One request. Free.

The response includes the same core fields as OpenSky (hex, callsign, lat, lon, altitude, speed, track, vertical rate, squawk) plus additional data: `dbFlags` (military/LADD/PIA bitfield), emergency status, barometric vs geometric altitude, wind calculations, navigation accuracy categories, and signal type (ADS-B, MLAT, TIS-B, etc.).

### Why this is better than OpenSky for military tracking

1. **MLAT is included.** Aircraft broadcasting Mode S without ADS-B positions get multilaterated. This catches a large category of military aircraft that OpenSky misses entirely.

2. **Dedicated `/mil` endpoint.** One call gets all military aircraft globally. No need to poll multiple bounding boxes and filter by callsign patterns. The database already classifies them.

3. **`/type/{code}` endpoint.** Query `C135` (KC-135 tankers), `P8` (P-8A Poseidon), `E3` (AWACS), `K35R` (KC-135R), `E6` (TACAMO), `RC135` directly. No callsign regex needed.

4. **`/squawk/{code}` endpoint.** Squawk 7700 (emergency), 7600 (comms failure), 7500 (hijack) are immediate alerts. Military emergency squawks during active operations are high-value signals.

5. **Unfiltered policy.** They explicitly state they "will never filter or obfuscate MLAT results." ADS-B Exchange (via RapidAPI) makes the same claim, but Airplanes.live is free and rate-limited at 1/second rather than 10,000/month.

6. **1 request/second rate limit.** That's 86,400 requests/day. Versus OpenSky's 4,000 or ADS-B Exchange's 333.

### Limitations

1. **No SLA, no uptime guarantee.** It's a community project. Could go down during peak load (like, during a major war when everyone is watching).

2. **Non-commercial use only.** Fine for our personal dashboard.

3. **"Access does not currently require a feeder. That might change."** They may gate the API behind feeding data in the future. Worth setting up a feeder if you have a Raspberry Pi and an ADS-B antenna.

4. **Coverage still depends on receiver density.** Same physics as OpenSky — if there's no receiver in range, there's no data. Middle East coverage is better than OpenSky because of MLAT computation, but still thinner than Europe/US.

5. **250nm radius limit on point queries.** Fine for our use — 250nm circles centred on key locations cover the areas we need.

### Verdict

**Use as the primary military flight tracking source.** Poll `/mil` every 10–15 seconds for a global military picture. Supplement with `/point/{lat}/{lon}/250` queries over the 4 Middle East regions every 30 seconds for total coverage including civilian traffic (airspace closures, diversions). The rate limit of 1/second means we can comfortably do all of this simultaneously.

---

## Source 3: ADS-B Exchange via RapidAPI ($10/month)

- **URL:** https://rapidapi.com/adsbx/api/adsbexchange-com1
- **Direct purchase:** Not available. Personal/hobbyist access is RapidAPI only. Enterprise is contact-sales.
- **Auth:** RapidAPI key
- **Cost:** $10/month for 10,000 requests. Overage $0.0015/request.
- **Rate limit:** No per-second limit, but 10,000 requests/month total.
- **Bandwidth:** 10GB/month included.

### What it gives us

ADS-B Exchange is the world's largest co-op of unfiltered ADS-B/Mode S/MLAT feeders. Functionally equivalent data to Airplanes.live — unfiltered, includes MLAT, includes military. The key difference is the delivery mechanism and price.

**Endpoints (via RapidAPI):**
- Bounding box / radius queries for aircraft positions
- ICAO hex lookup
- Callsign lookup
- Military filter available

The data quality is excellent. ADS-B Exchange has more feeders than Airplanes.live and has been operating longer, so coverage may be marginally better in some regions.

### The budget problem

10,000 requests/month = ~333/day = ~14/hour.

To monitor 4 Middle East regions at 60-second intervals: 4 × 1440 = 5,760 requests/day. **Blows through the monthly budget in under 2 days.**

Even a single region at 60-second polling uses 43,200 requests/month — 4x the budget.

To stay within budget you'd need to poll one region every ~4 minutes. During an active war with strikes landing every few hours, that's too slow for a "real-time" dashboard.

### When it's still worth having

ADS-B Exchange has the largest receiver network and the most established data pipeline. Airplanes.live is newer and smaller. If Airplanes.live goes down during peak conflict load (plausible), ADS-B Exchange via RapidAPI is the fallback.

It's also useful as an **event-triggered source** rather than a continuous poll. When another pipeline source fires an alert (FIRMS thermal anomaly, ACLED event, BGP outage, GDELT strike report), trigger an ADS-B Exchange query for the area. At 20–50 targeted lookups per day, 10,000/month is plenty.

### Verdict

**Optional. Get it if you want redundancy.** With Airplanes.live available for free, ADS-B Exchange's $10/month buys you a backup and confirmation source, not your primary data feed.

---

## Recommended architecture

```
┌──────────────────────────────────────────────────┐
│              CONTINUOUS POLLING LAYER             │
│                                                  │
│  Airplanes.live /mil        → every 10-15 sec   │
│  Airplanes.live /point × 4  → every 30 sec      │
│  OpenSky /states/all × 4    → every 90 sec      │
│                                                  │
│  Total: ~8 req/min (well within all limits)      │
└───────────────────────┬──────────────────────────┘
                        │
                        ▼
┌──────────────────────────────────────────────────┐
│              EVENT CORRELATION ENGINE             │
│                                                  │
│  Cross-reference with:                           │
│  - FIRMS thermal anomalies (strike detection)    │
│  - ACLED / GDELT conflict events                 │
│  - IODA / Cloudflare internet outages            │
│  - BGP route withdrawals                         │
│  - Shodan Streaming (ICS/SCADA disruption)       │
│  - AIS / Global Fishing Watch (maritime)         │
└───────────────────────┬──────────────────────────┘
                        │
                        ▼ (on alert trigger only)
┌──────────────────────────────────────────────────┐
│           EVENT-TRIGGERED ENRICHMENT             │
│                                                  │
│  ADS-B Exchange (RapidAPI) → targeted lookup     │
│  ~20-50 queries/day = ~600-1500/month            │
│  Confirms Airplanes.live data, fills gaps        │
│                                                  │
│  Budget: 10,000/month — comfortable headroom     │
└──────────────────────────────────────────────────┘
```

### Polling schedule detail

| Source | Endpoint | Interval | Requests/day | Purpose |
|--------|----------|----------|-------------|---------|
| Airplanes.live | `/v2/mil` | 15 sec | 5,760 | Global military picture |
| Airplanes.live | `/v2/point` × 4 regions | 30 sec | 11,520 | Total Middle East coverage |
| Airplanes.live | `/v2/type/C135` | 60 sec | 1,440 | Tanker orbit detection |
| Airplanes.live | `/v2/squawk/7700` | 60 sec | 1,440 | Emergency squawk alerts |
| OpenSky | `/states/all` × 4 bboxes | 90 sec | 3,840 | Secondary/cross-reference |
| ADS-B Exchange | On-demand | Event-triggered | ~30-50 | Enrichment/confirmation |

Total Airplanes.live: ~20,160 requests/day = ~0.23 req/sec (well under 1/sec limit).
Total OpenSky: 3,840 credits/day (under 4,000 limit).
Total ADS-B Exchange: ~30-50/day (~900-1500/month, well under 10,000 limit).

### Bounding box centres for Airplanes.live point queries

Airplanes.live uses point + radius (up to 250nm), not bounding boxes. These centres cover the key areas:

| Region | Centre lat | Centre lon | Radius | Coverage |
|--------|-----------|-----------|--------|----------|
| Iran (central) | 32.5 | 53.0 | 250 nm | Most of Iran including Tehran, Isfahan, nuclear sites |
| Israel/Lebanon/Syria | 33.0 | 36.0 | 200 nm | Full Levant including IDF operations area |
| Persian Gulf | 26.5 | 52.0 | 250 nm | Hormuz strait, Bahrain (5th Fleet), Qatar (Al Udeid), UAE |
| Red Sea/Yemen | 15.0 | 43.0 | 250 nm | Bab-el-Mandeb, Houthi territory, Red Sea shipping lane |

### Military aircraft type codes to watch

| ICAO type | Aircraft | Intelligence value |
|-----------|----------|--------------------|
| C135 | RC-135 Rivet Joint | SIGINT reconnaissance |
| K35R | KC-135R Stratotanker | Aerial refuelling (racetrack = active ops) |
| KC10 | KC-10 Extender | Aerial refuelling |
| KC46 | KC-46A Pegasus | Next-gen tanker |
| E3 | E-3 Sentry AWACS | Airborne command & control |
| E6 | E-6B Mercury (TACAMO) | Nuclear C3 relay — presence = highest alert |
| P8 | P-8A Poseidon | Maritime patrol / ASW |
| E8 | E-8C JSTARS | Ground surveillance radar |
| RQ4 | RQ-4 Global Hawk | High-altitude ISR drone |
| MQ9 | MQ-9 Reaper | Armed ISR drone |
| C17 | C-17 Globemaster | Strategic airlift (surge = force deployment) |
| C5 | C-5 Galaxy | Strategic airlift |
| B52 | B-52 Stratofortress | Strategic bomber |
| B1 | B-1B Lancer | Conventional bomber |
| B2 | B-2 Spirit | Stealth bomber (rare on ADS-B) |
| F35 | F-35 variants | Stealth fighter (rarely broadcast) |
| F22 | F-22 Raptor | Air superiority (deployed to Ovda, Israel on Feb 24) |

### Callsign patterns to flag

| Pattern | Meaning |
|---------|---------|
| `REACH*` | US Air Mobility Command (C-17/C-5 airlift) |
| `RCH*` | Same, abbreviated |
| `JAKE*` / `ETHYL*` | KC-135 tanker callsigns |
| `NCHO*` | P-8A Poseidon maritime patrol |
| `FORTE*` | RQ-4 Global Hawk ISR |
| `HOMER*` | E-6B TACAMO (nuclear comms) |
| `SENTRY*` | E-3 AWACS |
| `GORDO*` | EC-130 Compass Call (electronic attack) |
| `IAF*` / `ISR*` | Israeli Air Force |
| `IRGC*` | Iranian Revolutionary Guard (unlikely on ADS-B) |

---

## Summary

| | OpenSky | Airplanes.live | ADS-B Exchange (RapidAPI) |
|---|---------|----------------|--------------------------|
| **Cost** | Free | Free | $10/month |
| **Auth** | OAuth2 (client credentials) | None | RapidAPI key |
| **Rate limit** | 4,000 credits/day | 1 req/sec (~86K/day) | 10,000 req/month (~333/day) |
| **MLAT** | No | **Yes** | Yes |
| **Military filter** | No (must filter by callsign/type) | **Yes (`/mil` endpoint)** | Yes |
| **Type filter** | No | **Yes (`/type/{code}`)** | Limited |
| **Squawk filter** | No | **Yes (`/squawk/{code}`)** | Limited |
| **Filtering policy** | Unfiltered (no censorship) | Unfiltered + MLAT never obfuscated | Unfiltered |
| **Coverage** | Good (Europe/US), thin (Middle East) | Good, growing | Best (largest feeder network) |
| **Reliability** | Non-profit, established since 2012 | Community project, no SLA | Commercial via RapidAPI |
| **Our role** | Secondary/cross-reference | **Primary military tracking** | Fallback/event-triggered |

**Bottom line:** Airplanes.live gives us everything ADS-B Exchange does — unfiltered, MLAT-computed, with a military endpoint — for free, at 260x the request budget. OpenSky provides a solid secondary feed. ADS-B Exchange at $10/month is worth having as a redundancy play but is no longer the primary source. Total cost for flight tracking: $0–$10/month.
