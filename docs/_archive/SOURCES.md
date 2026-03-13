# OSINT Warfare Monitor — Data Sources Reference

> **For:** Claude Code building a Rust + SvelteKit conflict monitoring dashboard.
> **Operator context:** Shodan Academic (Lifetime) plan already owned. Tracking kinetic + cyber warfare globally. **Primary focus: Iran–Israel war (Feb 2026 escalation), Gulf states, Red Sea/Hormuz maritime.** Secondary: Ukraine/Russia, Sudan, Taiwan Strait, Myanmar.

---

## Tier 1 — Free, high value, integrate first

### Shodan (Academic Lifetime — already owned)

Internet-wide device and service scanning. Identifies exposed systems, ICS/SCADA, certificates, software versions, vulnerabilities across all internet-connected devices globally.

- **API docs**: https://developer.shodan.io/api
- **Python library**: https://github.com/achillean/shodan-python (`pip install shodan`)
- **InternetDB (free, no auth)**: https://internetdb.shodan.io — bulk IP enrichment, no credits needed
- **Signup**: Already have Academic plan
- **Cost**: £0 (lifetime)
- **Credits**: 200,000 query/month, 65,536 scan, 131,071 monitored IPs

**Key endpoints and credit costs:**

| Endpoint | Credits | Use case |
|----------|---------|----------|
| `api.count(query, facets)` | 0 | Country device counts, port/org/ASN breakdowns — use for periodic health checks |
| `api.host(ip)` | 0 | Individual IP lookup |
| `api.search_cursor(query)` | 1 per 100 results | Bulk search with auto-pagination |
| `api.stream.countries(['UA','RU','IL','PS','SY'])` | 0 | **Real-time streaming** — primary ingestion method, zero cost |
| `shodan download` | 1 per 100 results | Bulk download up to 20M results/month |
| InternetDB (`internetdb.shodan.io/{ip}`) | 0, no auth | Fast bulk IP screening — ports, vulns, hostnames |

**Conflict-relevant queries:**
- `tag:ics country:UA` — all Ukrainian ICS devices
- `port:502 country:UA` — Modbus (power/water SCADA)
- `port:102 country:RU` — Siemens S7 PLCs
- `port:20000 country:UA` — DNP3 (power grid)
- `port:47808 country:UA` — BACnet (building automation)
- `has_screenshot:true country:UA` — exposed cameras and HMIs
- `ssl.cert.subject.cn:*.gov.ua` — Ukrainian government TLS infrastructure

**Streaming API detail:**
The Streaming API is a persistent HTTP stream (newline-delimited JSON). Each line is a complete banner object. Filter by country, port, ASN, or vulnerability. This is the highest-value zero-cost data source — it provides a continuous feed of every device Shodan discovers in your filtered countries. In Rust, consume with `reqwest` streaming + `serde_json` line-by-line deserialization.

**Companion tools (reference only):**
- Shomon (https://github.com/tanprathan/Shomon) — Shodan stream alerting patterns
- ShodanX (https://github.com/RevoltSecurities/ShodanX) — advanced recon with SSL fingerprinting
- Shodan ICS queries (https://github.com/EC-429/Shodan_ICS) — curated SCADA search list
- MCP Server (https://github.com/ADEOSec/mcp-shodan) — Shodan MCP integration for Claude

---

### NASA FIRMS (Fire Information for Resource Management System)

Global active fire and thermal anomaly detection from MODIS (1km) and VIIRS (375m) satellites. Detects fires within ~3 hours of satellite overpass. Artillery exchanges, fuel depot strikes, and ammunition dumps produce distinct thermal signatures distinguishable by Fire Radiative Power (FRP) values.

- **API docs**: https://firms.modaps.eosdis.nasa.gov/api/area/
- **Map key request**: https://firms.modaps.eosdis.nasa.gov/api/map_key/
- **Signup**: https://urs.earthdata.nasa.gov/users/new (free NASA Earthdata account)
- **Cost**: Free
- **Rate limit**: 5,000 transactions per 10 minutes

**API pattern:**
```
GET https://firms.modaps.eosdis.nasa.gov/api/area/csv/{MAP_KEY}/VIIRS_SNPP_NRT/{west},{south},{east},{north}/{days}
```

Returns CSV with columns: latitude, longitude, bright_ti4, scan, track, acq_date, acq_time, satellite, instrument, confidence, version, bright_ti5, frp, daynight, type

**Key parameters:**
- Satellite options: `VIIRS_SNPP_NRT` (375m, preferred), `VIIRS_NOAA20_NRT`, `MODIS_NRT` (1km)
- Days: 1-10 (NRT data), or use `/date/{YYYY-MM-DD}` for specific dates
- Output formats: CSV, JSON, KML, SHP
- Country filter alternative: `GET .../api/country/csv/{MAP_KEY}/VIIRS_SNPP_NRT/UKR/1`

**Conflict bounding boxes:**
- Ukraine: `22.0,44.0,40.5,52.5`
- Middle East (Levant → Iran + Yemen): `32.0,12.0,60.0,42.0`
- Sudan: `21.0,3.0,39.0,23.0`
- Myanmar: `92.0,9.5,101.5,28.5`

**Intelligence notes:**
- FRP (Fire Radiative Power) in MW — conflict fires typically show high FRP with sudden onset
- `confidence` field: `nominal`, `low`, `high` — filter for `high` in conflict analysis
- Compare against 7-day rolling baseline to detect anomalous spikes
- Bellingcat demonstrated using FIRMS to track Russian artillery patterns in Ukraine

---

### ACLED (Armed Conflict Location & Event Data)

Human-coded structured conflict event data: battles, explosions/remote violence, violence against civilians, protests, riots, strategic developments. Covers all countries. Updated weekly. The gold standard for conflict data — each event has date, precise location, actors, fatalities, event type, sub-type, and source references.

- **API docs**: https://apidocs.acleddata.com/
- **Signup**: https://developer.acleddata.com/
- **Dashboard / explorer**: https://acleddata.com/dashboard/
- **Cost**: Free (requires registration)
- **Auth**: Email + API key, generate access token (24-hour validity) via `/token` endpoint
- **Rate limit**: 500 requests per minute, 5,000 rows per response (paginate with `page` param)

**API pattern:**
```
GET https://api.acleddata.com/acled/read?key={API_KEY}&email={EMAIL}&event_type=Battles&country=Ukraine&event_date={YYYY-MM-DD}&event_date_where=>&limit=5000
```

**Key filters:**
- `event_type`: Battles, Explosions/Remote violence, Violence against civilians, Protests, Riots, Strategic developments
- `sub_event_type`: Armed clash, Air/drone strike, Shelling/artillery/missile attack, Suicide bomb, etc.
- `country`, `region`, `admin1` (province), `admin2` (district)
- `event_date`, `event_date_where` (`>`, `<`, `BETWEEN`)
- `actor1`, `actor2` — named actor filtering
- `fatalities`, `fatalities_where` — casualty filtering

**Response fields:** event_id, event_date, event_type, sub_event_type, actor1, actor2, country, admin1, admin2, admin3, location, latitude, longitude, fatalities, source, source_scale, notes, tags, timestamp

**Intelligence notes:**
- ~1 week data lag (human coding takes time) — complement with GDELT for near-real-time
- Source reliability is high — professional coders cross-reference multiple sources
- `source_scale` indicates geographic coverage of the reporting source
- `tags` field contains conflict-specific metadata

---

### GDELT (Global Database of Events, Language, and Tone)

Monitors news media from 65 languages worldwide, auto-coding events using CAMEO taxonomy. Updates every 15 minutes. Provides both event data and article-level data with tone/sentiment scoring and geolocation.

- **Doc API docs**: https://blog.gdeltproject.org/gdelt-doc-2-0-api-drafts/
- **GEO API docs**: https://blog.gdeltproject.org/gdelt-geo-2-0-api-drafts/ (returns GeoJSON — ideal for mapping)
- **Python client**: https://github.com/alex9smith/gdelt-doc-api (`pip install gdeltdoc`)
- **TV/News explorer**: https://api.gdeltproject.org/api/v2/summary/summary
- **Signup**: None required
- **Cost**: Completely free
- **Rate limit**: 250 records per request, rolling 3-month window for Doc API

**Doc API pattern:**
```
GET https://api.gdeltproject.org/api/v2/doc/doc?query=ukraine+artillery&mode=ArtList&maxrecords=250&format=json
```

**GEO API pattern (returns GeoJSON directly):**
```
GET https://api.gdeltproject.org/api/v2/geo/geo?query=ukraine+war&format=GeoJSON
```

**Key parameters:**
- `query`: Boolean search (AND/OR/NOT), phrase search with quotes
- `mode`: `ArtList` (articles), `TimelineVol` (volume timeline), `TimelineTone` (sentiment), `ToneChart`
- `sourcecountry`: 2-letter FIPS code filter
- `sourcelang`: Language filter (e.g., `english`, `russian`, `arabic`)
- `timespan`: e.g., `7d`, `30d`, `3m`
- `format`: `json`, `csv`, `html`, `GeoJSON` (GEO API)

**CAMEO event codes relevant to conflict:**
- 18: Assault
- 19: Fight (use of conventional military force)
- 20: Use unconventional mass violence
- 190: Use conventional military force (broad)
- 194: Occupy territory
- 195: Fight with small arms

**Intelligence notes:**
- Auto-coded (lower accuracy than ACLED) but 15-minute latency vs ACLED's weekly lag
- GEO API is perfect for map overlays — returns ready-made GeoJSON
- Tone scores range from -100 to +100 — sharp negative swings correlate with conflict escalation
- Cross-reference GDELT events with ACLED for verification (auto-coded + human-coded convergence = high confidence)

---

### Cloudflare Radar

Internet traffic anomalies, BGP routing changes, outage detection/classification, DDoS attack trends. Built on Cloudflare's global network processing 63M+ HTTP requests/second.

- **API docs**: https://developers.cloudflare.com/api/resources/radar/
- **Dashboard**: https://radar.cloudflare.com
- **Outage center**: https://radar.cloudflare.com/outage-center
- **Signup**: https://dash.cloudflare.com/sign-up (free Cloudflare account)
- **Cost**: Free API with bearer token
- **Rate limit**: Varies by endpoint, generally generous for free tier

**Key endpoints:**
```
# Internet outages
GET https://api.cloudflare.com/client/v4/radar/annotations/outages?dateRange=7d&location=UA
# Headers: Authorization: Bearer {token}

# Traffic anomalies by location
GET https://api.cloudflare.com/client/v4/radar/traffic_anomalies?location=UA&dateRange=30d

# BGP route leaks and hijacks
GET https://api.cloudflare.com/client/v4/radar/bgp/leaks/events?dateRange=7d

# HTTP traffic timeseries by country
GET https://api.cloudflare.com/client/v4/radar/http/timeseries?location=UA&dateRange=7d
```

**Outage classifications:** `government_ordered` (shutdowns), `cable_cut`, `power_outage`, `technical`, `unknown` — the cause classification is extremely valuable for distinguishing deliberate infrastructure targeting from incidental failures.

**Intelligence notes:**
- Real-time visibility into internet health per country/ASN
- Outage center provides human-annotated explanations of major outages
- BGP leak/hijack detection catches routing attacks
- Traffic anomaly detection can reveal DDoS campaigns and coordinated shutdowns

---

### OpenSky Network

Real-time and historical aircraft state vectors from a global network of ADS-B receivers. Provides position, velocity, heading, altitude, and aircraft identification for all transponder-equipped aircraft.

- **REST API docs**: https://openskynetwork.github.io/opensky-api/rest.html
- **Python API**: https://github.com/openskynetwork/opensky-api
- **Signup**: https://opensky-network.org/register (free, OAuth2 since March 2025)
- **Cost**: Free
- **Rate limit**: 4,000 credits/day anonymous, 8,000/day registered

**Key endpoints:**
```
# All aircraft state vectors (global or bounded)
GET https://opensky-network.org/api/states/all?lamin=44&lomin=22&lamax=52.5&lomax=40.5
# (Ukraine bounding box)

# Flights by aircraft (by ICAO24 address)
GET https://opensky-network.org/api/flights/aircraft?icao24={addr}&begin={unix}&end={unix}

# Arrivals/departures by airport
GET https://opensky-network.org/api/flights/arrival?airport=UKBB&begin={unix}&end={unix}
```

**State vector fields:** icao24, callsign, origin_country, time_position, last_contact, longitude, latitude, baro_altitude, on_ground, velocity, true_track, vertical_rate, geo_altitude, squawk, spi, position_source, category

**Military aircraft identification:**
Track by ICAO type designators: `C135` (RC-135 recon), `P8` (P-8A maritime patrol), `E3` (AWACS), `K35R` (KC-135 tanker), `EUFI` (Eurofighter), `F16` (Fighting Falcon), `A30B` (A-10 Warthog)

KC-135 tanker aircraft flying racetrack/orbit patterns indicate active aerial refueling operations — a strong indicator of ongoing air operations.

**Intelligence notes:**
- Provides raw unfiltered data (unlike FlightRadar24 which censors military traffic at government request)
- ADS-B spoofing is active in conflict zones — cross-verify data
- Credit budget: each `/states/all` call costs 4 credits; polling every 10 seconds burns ~34,560/day — use bounding boxes to reduce scope
- Historical data available for registered users (trajectory waypoints)

---

### IODA (Internet Outage Detection and Analysis)

Macroscopic internet outage detection at country, region, and AS (network) levels. Combines three independent signal types for high-confidence detection: BGP routing data (~500 monitors), internet background radiation (UCSD Network Telescope), and active probing of routable IPv4 space.

- **Dashboard**: https://ioda.live
- **API docs**: https://api.ioda.inetintel.cc.gatech.edu/v2/ (documented at the dashboard site)
- **Data access**: https://ioda.inetintel.cc.gatech.edu/data-access
- **Signup**: None for dashboard; API access may require institutional contact
- **Cost**: Free, open data
- **Rate limit**: Not formally documented; moderate use is fine

**API pattern:**
```
# Country-level signals
GET https://api.ioda.inetintel.cc.gatech.edu/v2/signals/raw/country/UA?from={unix}&until={unix}

# ASN-level signals
GET https://api.ioda.inetintel.cc.gatech.edu/v2/signals/raw/asn/6849?from={unix}&until={unix}
# (AS6849 = Ukrtelecom)
```

**Three signal types:**
1. **BGP**: Active route announcements — drops indicate prefix withdrawals (infrastructure destruction or deliberate blackholing)
2. **Active Probing**: Responses to ICMP/TCP probes to all routable IPv4 — drops indicate unreachable hosts
3. **Darknet/Telescope**: Unsolicited traffic from misconfigured devices — drops indicate entire networks going offline

**Intelligence notes:**
- Detected the Kyivstar compromise (Dec 2023) in real-time via all three signal types
- ASN-level granularity is critical — country-level masks localised outages
- Provides historical data back to 2013 — build baselines for conflict-zone networks
- Complements Cloudflare Radar: IODA provides network-level granularity, Cloudflare provides traffic-level

---

### BGPStream

Live and historical BGP analysis from RouteViews and RIPE RIS collectors. Detects prefix hijacks, route leaks, origin changes, and path anomalies.

- **Docs**: https://bgpstream.caida.org
- **Python bindings**: PyBGPStream (`pip install pybgpstream`)
- **Live stream**: https://bgpstream.crosscloud.net/
- **Signup**: None
- **Cost**: Free, open-source
- **Rate limit**: None (live stream is continuous)

**Python usage:**
```python
import pybgpstream
stream = pybgpstream.BGPStream(
    from_time="2024-01-01 00:00:00",
    until_time="2024-01-02 00:00:00",
    collectors=["rrc00", "route-views2"],
    record_type="updates",
    filter="prefix more 91.198.0.0/16"  # Ukrainian prefix space
)
for rec in stream.records():
    for elem in rec:
        print(elem.type, elem.fields)
```

**For Rust:** No native Rust bindings exist. Options:
1. Call PyBGPStream from a Python sidecar process
2. Use the BGPStream Broker API (REST) for historical queries: `https://broker.bgpstream.caida.org/v2/...`
3. Connect directly to RIPE RIS Live WebSocket: `wss://ris-live.ripe.net/v1/ws/`

**RIPE RIS Live (recommended for Rust):**
```
wss://ris-live.ripe.net/v1/ws/
Subscribe message: {"type":"ris_subscribe","data":{"prefix":"0.0.0.0/0","type":"UPDATE","socketOptions":{"includeRaw":false}}}
```
Filter by prefix, origin ASN, or peer ASN. Native WebSocket — straightforward in Rust with `tokio-tungstenite`.

**Key ASNs to monitor:**

*Ukraine/Russia:*

| ASN | Operator | Country |
|-----|----------|---------|
| AS6849 | Ukrtelecom | Ukraine |
| AS15895 | Kyivstar | Ukraine |
| AS21497 | Lifecell | Ukraine |
| AS13249 | Datagroup | Ukraine |
| AS12389 | Rostelecom | Russia |
| AS8402 | Beeline | Russia |
| AS25513 | MTS | Russia |
| AS31133 | MegaFon | Russia |

*Iran (gateways + major ISPs):*

| ASN | Operator | Notes |
|-----|----------|-------|
| AS12880 | ITC | International gateway (state-controlled) |
| AS48159 | TIC | International gateway (state-controlled) |
| AS6736 | IPM | Academic/research gateway |
| AS58224 | TCI | Fixed-line backbone |
| AS197207 | MCCI | Largest mobile (shut down first in blackouts) |
| AS44244 | IranCell | Second mobile |
| AS57218 | RighTel | Third mobile |
| AS16322 | ParsOnline | Major consumer ISP |
| AS31549 | Shatel | Major consumer ISP |

*Israel + Gulf:*

| ASN | Operator | Country |
|-----|----------|---------|
| AS378 | Bezeq | Israel |
| AS9116 | Partner | Israel |
| AS8551 | Bezeq International | Israel |
| AS8966 | Etisalat | UAE |
| AS5384 | du | UAE |
| AS59605 | Zain | Bahrain |
| AS8781 | Ooredoo | Qatar |
| AS30873 | YemenNet | Yemen |

**Companion tools (reference):**
- RIPE BGPlay (visual): https://stat.ripe.net/bgplay
- ARTEMIS (real-time hijack detection): https://bgpartemis.org

---

### GeoConfirmed

Community-verified geolocated conflict events sourced from social media. Each event has precise coordinates, source media links, verification status, and conflict categorisation. Covers Ukraine, Israel/Palestine, Myanmar, Sudan, and other active conflicts.

- **Website**: https://geoconfirmed.org
- **Data extraction library**: https://github.com/conflict-investigations/osint-geo-extractor (Python — extracts from GeoConfirmed, Bellingcat, CIR, DefMon3, Texty.org.ua)
- **Signup**: None for data access via extractor library
- **Cost**: Free
- **Rate limit**: Be respectful; no formal API — data extracted from public map feeds

**Python usage:**
```python
from osint_geo_extractor import GeoConfirmed
gc = GeoConfirmed()
events = gc.fetch_events()  # Returns list of Event objects with lat, lon, date, description, source_url, etc.
```

**Intelligence notes:**
- Higher confidence than auto-coded sources (community verification)
- Includes source media links (photos/video) for each event
- The osint-geo-extractor library also pulls from Bellingcat's investigation maps, Centre for Information Resilience (CIR), and DefMon3 — gives you multiple verified OSINT databases in one library
- No formal API — scrape/extract pattern means data format may change; pin the library version

---

### OONI (Open Observatory of Network Interference)

Network censorship measurements from the OONI Probe app running on volunteer devices globally. 2+ billion measurements from 241 countries since 2012. Detects website blocking, app blocking, instant messaging blocking, and network manipulation techniques (DNS injection, TCP reset, HTTP blocking).

- **API docs**: https://api.ooni.io/ (Explorer API)
- **Explorer**: https://explorer.ooni.org
- **Signup**: None
- **Cost**: Free, all data is open
- **Rate limit**: Not formally documented; moderate use expected

**API pattern:**
```
# Recent measurements for a country
GET https://api.ooni.io/api/v1/measurements?probe_cc=UA&test_name=web_connectivity&since=2024-01-01&limit=100

# Aggregated censorship data
GET https://api.ooni.io/api/v1/aggregation?probe_cc=RU&test_name=web_connectivity&input=https://twitter.com&since=2024-01-01&until=2024-12-31&axis_x=measurement_start_day
```

**Test types:**
- `web_connectivity` — website blocking detection
- `telegram` — Telegram accessibility
- `whatsapp` — WhatsApp accessibility  
- `signal` — Signal accessibility
- `tor` — Tor network reachability
- `ndt` — Network performance

**Intelligence notes:**
- Censorship intensifies during conflicts — track over time to correlate with military operations
- Shows *how* blocking is implemented (DNS, TCP, HTTP) which indicates sophistication level
- Russia, Iran, and Myanmar are heavily measured with good probe coverage

---

### Global Fishing Watch

AIS vessel tracking combined with Sentinel-1 SAR satellite imagery and ML-based dark vessel detection. Tracks ~550,000 vessels. Detects vessels not broadcasting AIS ("dark vessels"). Catalogues AIS gap events (6+ million gaps documented).

- **API portal**: https://globalfishingwatch.org/our-apis/
- **API docs**: https://globalfishingwatch.org/our-apis/documentation
- **Signup**: https://globalfishingwatch.org/our-apis/tokens (free registration)
- **Cost**: Free
- **Rate limit**: Reasonable use; token-authenticated

**Key capabilities:**
- Vessel track histories with AIS positions
- AIS gap events (vessel turns off transponder — 6M+ documented)
- Dark vessel detections from SAR imagery (vessels with no AIS broadcasting)
- Port visit histories
- Encounter events (vessel-to-vessel meetings at sea)

**Conflict relevance:**
- Russia's shadow tanker fleet disables AIS to evade sanctions — documented 6x more AIS gaps than European vessels
- Track vessel behaviour near chokepoints: Bab-el-Mandeb (190+ Houthi attacks since Oct 2023), Strait of Hormuz, Taiwan Strait
- Dark vessel detection reveals vessels deliberately hiding from tracking systems
- Encounter events can reveal at-sea transfers (sanctions evasion, arms transfers)

**Maritime chokepoint bounding boxes:**
- Bab-el-Mandeb: `43.0,12.0,44.0,13.0`
- Strait of Hormuz: `54.0,25.5,57.0,27.0`
- Suez Canal: `32.0,29.5,33.0,31.5`
- Taiwan Strait: `117.0,22.0,122.0,26.0`
- Malacca Strait: `99.0,1.0,104.5,4.5`

---

### AlienVault OTX (Open Threat Exchange)

Crowdsourced threat intelligence platform with 140,000+ participants. 19M+ indicators of compromise (IOCs) shared daily. Community "Pulses" contain curated IOC packages from APT campaigns.

- **API docs**: https://otx.alienvault.com/api
- **Python SDK**: `pip install OTXv2`
- **Signup**: https://otx.alienvault.com (free)
- **Cost**: Free
- **Rate limit**: 10,000 API calls/hour

**Key endpoints:**
```
# Get indicators for a Pulse
GET https://otx.alienvault.com/api/v1/pulses/{pulse_id}

# Search pulses by keyword
GET https://otx.alienvault.com/api/v1/search/pulses?q=ukraine+apt

# Get threat intel for an IP
GET https://otx.alienvault.com/api/v1/indicators/IPv4/{ip}/general
```

**Conflict-relevant APT groups to track pulses for:**
- APT28 / Fancy Bear (Russia/GRU)
- Sandworm / Voodoo Bear (Russia/GRU — Ukraine infrastructure attacks)
- APT29 / Cozy Bear (Russia/SVR)
- APT33 / Elfin (Iran)
- APT35 / Charming Kitten (Iran)
- Lazarus Group (North Korea)
- APT41 / Winnti (China)

---

### Certificate Transparency (crt.sh + Certstream)

crt.sh provides searchable Certificate Transparency logs for any domain. Certstream provides a real-time WebSocket feed of every newly issued TLS certificate globally.

- **crt.sh**: https://crt.sh — search interface, also has JSON API
- **Certstream**: https://certstream.calidog.io — real-time WebSocket stream (open-source)
- **Certstream source**: https://github.com/CaliDog/certstream-server
- **Signup**: None
- **Cost**: Free
- **Rate limit**: crt.sh is a public service (be gentle); Certstream is a live stream

**crt.sh API:**
```
# Search for certificates by domain
GET https://crt.sh/?q=%.gov.ua&output=json

# Search by organisation
GET https://crt.sh/?q=org:Ministry+of+Defence&output=json
```

**Certstream WebSocket:**
```
wss://certstream.calidog.io
```
Emits JSON for every new certificate. Filter client-side by domain patterns of interest (e.g., `.gov.ua`, `.mil.ru`, `.mod.gov.il`).

**Intelligence notes:**
- New certificates on military/government domains indicate new infrastructure being provisioned
- Phishing campaigns often generate certificates for lookalike domains (typosquatting)
- C2 (command and control) infrastructure often uses Let's Encrypt certificates — watch for suspicious patterns
- In Rust, connect to Certstream via `tokio-tungstenite` and filter the stream

---

## Tier 2 — Free or low cost, add second

### ADS-B Exchange

**The only publicly accessible source for unfiltered military aviation data.** Does not censor military, government, or FAA-blocked aircraft that FlightRadar24 hides at government request.

- **Website**: https://www.adsbexchange.com
- **API docs**: https://www.adsbexchange.com/data/ (paid tiers)
- **Free viewer**: https://globe.adsbexchange.com (use "Military" filter — no API needed)
- **Signup**: https://www.adsbexchange.com/data/
- **Cost**:
  - Free: Web viewer only (no API)
  - **Hobbyist: ~$10/month** — 1 req/sec, 5,000 aircraft per response, 250nm radius
  - Enthusiast: ~$40/month — 10 req/sec, global coverage, full history
- **Rate limit**: Per plan tier

**API pattern (paid):**
```
# Aircraft in bounding box
GET https://adsbexchange.com/api/aircraft/json/lat/{lat}/lon/{lon}/dist/{nm}
# Headers: api-auth: {key}

# Military aircraft filter
GET https://adsbexchange.com/api/aircraft/json/mil/
```

**Intelligence notes:**
- OSINT analysts used ADS-B Exchange to detect increased RC-135 and KC-135 activity near Ukraine before the February 2022 invasion
- The **$10/month hobbyist plan is the single best value-for-money paid OSINT source** for military aviation tracking
- Military filter at globe.adsbexchange.com is free for manual monitoring — only API access requires payment
- Combine with OpenSky (free) for redundancy — OpenSky has broader coverage but ADS-B Exchange has better military identification

---

### GreyNoise

Separates benign internet scanning "noise" from targeted threat activity. Identifies mass-scanning campaigns, worm propagation, and coordinated offensive scanning.

- **API docs**: https://docs.greynoise.io/reference/get_v3-community-ip
- **Free tool**: https://check.labs.greynoise.io (no auth, individual IP lookups)
- **Signup**: https://viz.greynoise.io/signup (free Community tier)
- **Cost**: 
  - Community: Free (limited — IP lookups, basic classification)
  - Business: From ~$500/year
- **Rate limit**: Community: 50 queries/day; paid: varies

**Community API:**
```
GET https://api.greynoise.io/v3/community/{ip}
# Headers: key: {api_key}
```
Returns: `noise` (boolean), `riot` (boolean — benign service like Cloudflare), `classification` (benign/malicious/unknown), `name`, `last_seen`

**Intelligence notes:**
- Complement Shodan data: GreyNoise tells you whether scanning activity hitting your monitored infrastructure is targeted or background noise
- Useful for triaging Shodan Streaming API output — if an IP scanning Ukrainian ICS is classified as mass-scanner, it's less interesting than a targeted probe
- Free check tool at check.labs.greynoise.io requires no auth — useful for ad-hoc investigation

---

### Copernicus / Sentinel Hub

Free satellite imagery from ESA. Sentinel-1 (SAR) sees through clouds at 5m resolution. Sentinel-2 provides 10m optical multispectral imagery. Both have 5-6 day revisit times.

- **Browser**: https://browser.dataspace.copernicus.eu
- **API docs**: https://documentation.dataspace.copernicus.eu/APIs/
- **Signup**: https://identity.dataspace.copernicus.eu/auth/realms/CDSE/login-actions/registration
- **Cost**: Free
- **Rate limit**: Token-based, generous for individual use

**What each satellite provides:**

| Satellite | Type | Resolution | Revisit | Best for |
|-----------|------|-----------|---------|----------|
| Sentinel-1 | SAR (radar) | 5m | 6 days | Sees through clouds; metal objects (vehicles, ships) appear bright; SAR interference patterns reveal electronic warfare jamming |
| Sentinel-2 | Optical | 10m | 5-6 days | Building damage, vegetation change, displacement camps, nighttime lights |

**Intelligence notes:**
- Better suited to manual/ad-hoc analysis than automated feeds
- For automated integration, consider linking to Copernicus Browser with preset bounding boxes rather than building full image processing pipeline
- SAR interference patterns from GPS/radar jamming are visible in Sentinel-1 data — a known technique for detecting electronic warfare activity
- Bellingcat's RS4OSINT guide: https://bellingcat.github.io/RS4OSINT/

---

## Tier 3 — Higher cost, integrate when needed

| Source | URL | Cost | What it provides |
|--------|-----|------|-----------------|
| **ADS-B Exchange Enthusiast** | adsbexchange.com/data | ~$40/mo | Global unfiltered military flight data, full API, historical |
| **Censys** | search.censys.io | Free limited; paid ~$300/mo | Internet scanning — 8x more services than Shodan, 92% live accuracy vs 68%. Fills gaps for specific investigations. |
| **MarineTraffic API** | marinetraffic.com/en/ais-api-services | From ~$100/mo | Professional vessel tracking, historical AIS, port intelligence |
| **SecurityTrails** | securitytrails.com | Free (50 queries/mo); paid plans | Historical DNS records, domain intelligence, WHOIS history |
| **Planet Labs** | planet.com | Commercial | Daily 3m global satellite imagery |
| **Maxar** | maxar.com | Commercial | Sub-metre satellite imagery (used by ISW, Sudan Conflict Observatory) |

---

## Link/embed sources (no API needed)

These provide ready-made intelligence products. Embed as iframes or link panels.

| Source | URL | What it provides |
|--------|-----|-----------------|
| **Liveuamap** | liveuamap.com / me.liveuamap.com | Real-time conflict event mapping, global coverage |
| **DeepState Map** | deepstatemap.live | Ukraine frontline positions, ~900K daily views, 350-400m accuracy |
| **ISW Daily Assessments** | understandingwar.org/backgrounder/ukraine-conflict-updates | Professional military analysis with control-of-terrain maps |
| **Oryx (equipment losses)** | oryxspioenkop.com | Photographically verified equipment losses, Ukraine/Russia |
| **FIRMS Fire Map** | firms.modaps.eosdis.nasa.gov/map | NASA's fire map viewer |
| **Cloudflare Radar Dashboard** | radar.cloudflare.com | Internet traffic and outage visualisation |
| **IODA Dashboard** | ioda.live | Internet outage visualisation |
| **RIPE BGPlay** | stat.ripe.net/bgplay | Visual BGP routing history for any prefix/ASN |
| **DefconLevel Flight Tracker** | defconlevel.com/military-flight-tracker | Curated military aviation dashboard |

---

## Region-specific bounding boxes and filters

### Ukraine/Russia
- **FIRMS bbox**: `22.0,44.0,40.5,52.5`
- **OpenSky bbox**: `lamin=44&lomin=22&lamax=52.5&lomax=40.5`
- **ACLED**: `country=Ukraine|Russia` or `region=Eastern+Europe`
- **Shodan stream**: `api.stream.countries(['UA','RU'])`

**Key ASNs:**

| ASN | Operator | Country |
|-----|----------|---------|
| AS6849 | Ukrtelecom | Ukraine |
| AS15895 | Kyivstar | Ukraine |
| AS21497 | Lifecell | Ukraine |
| AS13249 | Datagroup | Ukraine |
| AS12389 | Rostelecom | Russia |
| AS8402 | Beeline | Russia |
| AS25513 | MTS | Russia |
| AS31133 | MegaFon | Russia |

**OSINT accounts (vetted):** @Osinttechnical, @War_Mapper, @AndrewPerpetua, @Rebel44CZ, @DefMon3, @OAlexanderDK, @UAWeapons, @Noel_reports, @WarTranslated

**Telegram:** DeepState UA (700K+ subscribers), Ukrainian Air Force Official, General Staff of Ukraine

**Institutional:** ISW Ukraine updates (understandingwar.org), DeepState Map (deepstatemap.live, 900K daily views, 350-400m frontline accuracy), Oryx (oryxspioenkop.com — photographically verified equipment losses)

**GitHub data:** leedrake5/Russia-Ukraine (Oryx loss data in R/CSV)

### Middle East (Iran–Israel war, Gulf states, Red Sea, proxy fronts)

**Context (Feb 28, 2026):** Joint US-Israeli strikes on Iran (Operation "Epic Fury"), Iranian retaliation hitting Israel + 14 US bases across Gulf states, Strait of Hormuz effectively closed, Houthis resuming Red Sea shipping attacks, massive cyberattack knocked Iran to 4% internet connectivity. Conflict spans Iran, Israel, Yemen, Iraq, Syria, Bahrain, Qatar, Kuwait, UAE, Jordan, Saudi Arabia. This is the primary monitoring focus.

**FIRMS bounding boxes** (split for granularity):
- Iran: `44.0,25.0,63.5,40.0`
- Israel/Lebanon/Syria: `34.0,29.0,42.5,37.5`
- Yemen/Bab-el-Mandeb: `42.0,12.0,54.0,20.0`
- Gulf states (Bahrain/Qatar/Kuwait/UAE): `46.0,22.0,57.0,30.5`
- Iraq: `38.5,29.0,49.0,37.5`
- Full region (single query): `32.0,12.0,63.5,42.0`

**OpenSky bbox** (military aviation — critical for tracking tanker/ISR patterns):
- Iran: `lamin=25&lomin=44&lamax=40&lomax=63.5`
- Israel/Lebanon: `lamin=29&lomin=34&lamax=34&lomax=37`
- Persian Gulf: `lamin=22&lomin=46&lamax=30.5&lomax=57`
- Red Sea/Yemen: `lamin=12&lomin=38&lamax=20&lomax=46`

**ACLED**: `region=Middle+East` or `country=Iran|Israel|Yemen|Iraq|Syria|Lebanon`

**Shodan stream**: `api.stream.countries(['IR','IL','PS','SY','YE','IQ','LB','BH','QA','KW','AE','JO','SA'])`

**Shodan ICS queries (Iran — critical infrastructure)**:
- `tag:ics country:IR` — all Iranian ICS devices
- `port:502 country:IR` — Modbus (power/industrial SCADA)
- `port:102 country:IR` — Siemens S7 PLCs
- `port:20000 country:IR` — DNP3 (power grid)
- `port:47808 country:IR` — BACnet (building automation)
- `port:1911 country:IR` — Niagara Fox (building management)
- `ssl.cert.subject.cn:*.gov.ir` — Iranian government TLS infrastructure
- `ssl.cert.subject.cn:*.irgc.ir` — IRGC-associated domains
- `has_screenshot:true country:IR` — visual intelligence (cameras, HMIs)

**Key ASNs to monitor** (via Shodan Streaming, IODA, BGPStream/RIPE RIS Live):

*Iran (critical — currently at 4% connectivity due to combined cyberattack + state shutdown since Jan 8):*

| ASN | Operator | Type | Notes |
|-----|----------|------|-------|
| AS12880 | ITC (Information Technology Company) | International gateway | One of only two gateways to global internet; state-controlled |
| AS48159 | TIC (Telecommunication Infrastructure Company) | International gateway | Second gateway; also state-controlled via AS49666 |
| AS6736 | IPM (Institute for Research in Fundamental Sciences) | International gateway | Academic/research gateway |
| AS58224 | TCI (Iran Telecommunication Company) | Fixed-line backbone | Largest fixed-line operator |
| AS197207 | MCCI (Mobile Communication Company of Iran) | Cellular (largest) | First to be shut down in past blackouts |
| AS44244 | IranCell (MTN Irancell) | Cellular | Second largest mobile; also early shutdown target |
| AS57218 | RighTel | Cellular | Third mobile operator |
| AS16322 | ParsOnline | Fixed-line ISP | Major consumer ISP |
| AS31549 | Shatel | Fixed-line ISP | Major consumer ISP |
| AS43754 | Asiatech | Fixed-line ISP | |
| AS42337 | Respina | Fixed-line ISP | |
| AS56402 | Dadeh Gostar | Fixed-line ISP | |

**Iran internet architecture note:** ALL international traffic passes through just two state-controlled gateways (TIC and IPM). Since Jan 8 2026, Iran has operated a "stealth outage" — BGP routes show IPv4 as UP but actual traffic is blocked at the network level. IPv6 was fully withdrawn. Only whitelisted domestic services pass. You need BOTH BGP monitoring AND traffic analysis (IODA active probing + Cloudflare Radar) to see the full picture. NetBlocks confirmed 4% connectivity on Feb 28 during the strikes.

*Israel:*

| ASN | Operator | Notes |
|-----|----------|-------|
| AS378 | Bezeq | Largest fixed-line provider |
| AS8551 | Bezeq International | International transit |
| AS9116 | Partner (Orange) | Major mobile operator |
| AS12400 | Partner | Mobile |
| AS1680 | NV (013 Netvision) | Major ISP |
| AS47956 | Cellcom | Mobile operator |
| AS42925 | Pelephone | Mobile operator |
| AS8867 | HOT Telecom | Cable ISP |
| AS12849 | HOT-NET | Cable ISP |

*Gulf states (now directly in conflict zone — Iranian missiles struck all of these):*

| ASN | Operator | Country | Notes |
|-----|----------|---------|-------|
| AS8966 | Etisalat | UAE | Most interconnected regional AS; connected to 5 countries |
| AS5384 | Emirates Telecommunications (du) | UAE | Second UAE operator |
| AS59605 | Zain Bahrain | Bahrain | Regional hub — most Gulf traffic transits through Bahrain |
| AS5416 | Batelco | Bahrain | Major Bahraini operator; near US Fifth Fleet HQ |
| AS8781 | Ooredoo Qatar | Qatar | Near Al Udeid Air Base (struck by Iran) |
| AS48278 | Vodafone Qatar | Qatar | |
| AS21050 | Fast-Telco | Kuwait | Near Ali al-Salem Air Base (struck by Iran) |
| AS9155 | Qnet (Zain Kuwait) | Kuwait | |
| AS39891 | Saudi Telecom (STC) | Saudi Arabia | Largest regional carrier |
| AS25019 | Saudi Telecom | Saudi Arabia | |
| AS35753 | ITC Saudi | Saudi Arabia | |
| AS35819 | Mobily | Saudi Arabia | |
| AS48832 | Omantel | Oman | Critical — Hormuz chokepoint |

*Yemen (Houthis):*

| ASN | Operator | Notes |
|-----|----------|-------|
| AS30873 | YemenNet | State-controlled; Houthi-held territory |
| AS56258 | Aden Net | Government-controlled; southern Yemen |

**Maritime chokepoints** (for AIS/Global Fishing Watch — now the highest-priority monitoring):
- Strait of Hormuz (CLOSED as of Feb 28): `54.0,25.5,57.0,27.0`
- Bab-el-Mandeb (Houthi attack zone, attacks resuming): `43.0,12.0,44.0,13.5`
- Suez Canal (disrupted): `32.0,29.5,33.0,31.5`
- Eastern Mediterranean (Israeli naval zone): `33.0,31.0,36.0,34.0`
- Wider Red Sea: `36.0,12.0,43.5,28.0`

**UKMTO (UK Maritime Trade Operations):**
- **URL**: https://www.ukmto.org
- **What it provides**: Official maritime security warnings and incident reports for the Indian Ocean, Arabian Sea, Red Sea, and Gulf of Aden. UKMTO confirmed the Hormuz closure reports on Feb 28.
- **Integration**: RSS/scrape for real-time maritime threat alerts. No API — web scrape or manual monitoring.

**Key OSINT accounts / analysts (vetted — see misinformation warning below):**
- X/Twitter: @AuroraIntel (breaking military events, Middle East focus), @Faborsky (Iran specialist), @IntelCrab (conflict tracking), @Maborं (Israel military), @LucasFoxNews (military aviation), @Gerjon_ (shipping/maritime OSINT), @MT_Anderson (ADS-B military aviation tracking), @YWNReporter (Israel ground events), @JoeTruzman (FDD, IDF operations), @ELINTNews (electronic intelligence), @sentdefender (high-volume breaking news, but verify independently — misinformation risk flagged by researchers)
- **Misinformation warning**: ISD and University of Washington research documented that many "OSINT" accounts on X spread false, misleading, or AI-generated content during Iran-Israel escalations. Always cross-reference with institutional sources (ISW/CTP, ACLED, Reuters, AP). Accounts with "OSINT" in their name created in the last 2 years are generally unreliable per Bellingcat's Eliot Higgins.

**Telegram channels:**
- IDF Official (English) — Israeli military announcements
- Iran International — Farsi/English news, opposition-aligned
- Mossad Farsi channel (launched Feb 28 2026 to broadcast directly to Iranian citizens)
- Al Jazeera Arabic — fastest Arabic-language conflict updates
- Houthi military media (Ansar Allah) — Yemen military claims (propaganda; use for tracking claimed operations, not truth)
- Hezbollah media (Al Manar) — Lebanon front claims

**Institutional intelligence sources (high reliability):**
| Source | URL | What it provides | Update frequency |
|--------|-----|-----------------|-----------------|
| ISW/CTP Iran Updates | criticalthreats.org/analysis/iran-update | Professional military analysis with maps; now publishing special reports | Daily + special reports |
| Alma Research Center | israel-alma.org | Northern front analysis (Hezbollah, Syria, Iraq); publishing "Second Iran War" daily reports | Daily |
| Atlantic Council | atlanticcouncil.org/programs/middle-east-programs | Expert reaction and analysis pieces | Event-driven |
| CFR (Council on Foreign Relations) | cfr.org/global-conflict-tracker | Interactive conflict tracker + expert analysis | Daily |
| Chatham House | chathamhouse.org | Analysis of proxy dynamics and escalation scenarios | Event-driven |
| Middle East Eye | middleeasteye.net | Independent Middle East journalism, strong on Gulf states | Continuous |
| Al-Monitor | al-monitor.com | US-Middle East policy analysis | Daily |
| INSS (Israel National Security Studies) | inss.org.il | Israeli strategic analysis | Weekly + event-driven |

**Live dashboards and maps (embed/link panels):**
| Source | URL | What it provides |
|--------|-----|-----------------|
| **Iran Monitor** | iranmonitor.org | Real-time OSINT dashboard: news sentiment, X feeds, flight radar, prediction markets, internet connectivity |
| **Liveuamap Middle East** | me.liveuamap.com | Real-time conflict event mapping across entire Middle East |
| **NetBlocks** | netblocks.org | Real-time internet connectivity monitoring — confirmed Iran at 4% on Feb 28 |
| **SignalCockpit Iran Dashboard** | (LinkedIn-shared, search for latest link) | Aggregates official statements, OSINT indicators, market data |
| **FIRMS Fire Map** | firms.modaps.eosdis.nasa.gov/map | Preset to Middle East bbox for thermal anomaly monitoring |
| **Cloudflare Radar** | radar.cloudflare.com/ir / radar.cloudflare.com/il | Country-specific internet health dashboards |
| **IODA Iran** | ioda.live/country/IR | Iran-specific internet outage signals (BGP + probing + telescope) |

**Cyber warfare monitoring (Iran is the primary cyber target right now):**
- **NetBlocks** (netblocks.org): Confirmed Iran at 4% connectivity Feb 28. Primary source for real-time internet measurement during conflict.
- **IODA Iran report**: ioda.inetintel.cc.gatech.edu/reports/irans-nation-wide-internet-blackout-measurement-data-and-technical-observations/ — detailed technical analysis of Iran's shutdown architecture. Shows cellular operators (MCCI, IranCell, RighTel) are shut down first, then fixed-line follows hours later.
- **DataNarrative Iran analysis**: datanarrative.online — explains the "stealth outage" technique Iran uses (BGP shows UP, but traffic is blocked). You need both BGP and traffic-level monitoring to detect this.
- **OONI Iran probe data**: explorer.ooni.org filtered for `probe_cc=IR` — censorship measurements from volunteer probes inside Iran.
- **Israel INCD**: 26,500 cyber incidents in 2025 (55% increase). Iran's cyber operations surged 700% since June 2025, targeting Israeli power grids, hospitals, and civilian apps. Israel ranked #1 globally for geopolitically-motivated cyberattacks.

**APT groups active in this theater** (for AlienVault OTX pulse tracking):
- APT33 / Elfin / Refined Kitten (Iran → energy/aviation targeting)
- APT35 / Charming Kitten / Phosphorus (Iran → government/media)
- MuddyWater / Mercury (Iran/MOIS → Gulf state governments)
- Agrius / Pink Sandstorm (Iran → Israel destructive attacks)
- Crimson Sandstorm / Imperial Kitten (Iran → Israel maritime/logistics)
- OilRig / APT34 / Helix Kitten (Iran → Gulf state infrastructure)
- Unit 8200 / Israeli CNO (Israel → Iran infrastructure; attributed to Stuxnet lineage)

### Sudan
- **FIRMS bbox**: `21.0,3.0,39.0,23.0`
- **ACLED**: `country=Sudan`
- **Additional**: Sudan Conflict Observatory (state.gov), CIR Sudan Witness

### Taiwan Strait
- **AIS chokepoint**: `117.0,22.0,122.0,26.0`
- **OpenSky**: `lamin=22&lomin=117&lamax=26&lomax=122`
- **Additional**: CSIS Futures Lab AIS analysis, Taiwan MND ADIZ data

### Myanmar
- **FIRMS bbox**: `92.0,9.5,101.5,28.5`
- **ACLED**: `country=Myanmar`
- **Additional**: GeoConfirmed maps

---

## Useful GitHub repositories

| Repository | URL | What it does |
|-----------|-----|-------------|
| achillean/shodan-python | github.com/achillean/shodan-python | Official Shodan Python library |
| conflict-investigations/osint-geo-extractor | github.com/conflict-investigations/osint-geo-extractor | Extract geo data from GeoConfirmed, Bellingcat, CIR, DefMon3 |
| leedrake5/Russia-Ukraine | github.com/leedrake5/Russia-Ukraine | Oryx equipment loss tracking data (R/CSV) |
| alex9smith/gdelt-doc-api | github.com/alex9smith/gdelt-doc-api | Python client for GDELT 2.0 Doc API |
| tanprathan/Shomon | github.com/tanprathan/Shomon | Shodan real-time stream alerting patterns |
| EC-429/Shodan_ICS | github.com/EC-429/Shodan_ICS | Curated ICS/SCADA Shodan search lists |
| RevoltSecurities/ShodanX | github.com/RevoltSecurities/ShodanX | Advanced Shodan recon |
| openskynetwork/opensky-api | github.com/openskynetwork/opensky-api | Official OpenSky Python API |
| tegridydev/python-OSINT-notebook | github.com/tegridydev/python-OSINT-notebook | Comprehensive OSINT techniques notebook |
| smicallef/spiderfoot | github.com/smicallef/spiderfoot | 200+ module OSINT automation |
| intelowlproject/IntelOwl | github.com/intelowlproject/IntelOwl | Modular threat intelligence platform |
| ADEOSec/mcp-shodan | github.com/ADEOSec/mcp-shodan | Shodan MCP Server for Claude |

---

## Cost summary

| Source | Monthly | Annual | Value |
|--------|---------|--------|-------|
| Shodan Academic | £0 (lifetime) | £0 | ★★★★★ |
| NASA FIRMS | Free | Free | ★★★★★ |
| ACLED | Free | Free | ★★★★★ |
| GDELT | Free | Free | ★★★★☆ |
| Cloudflare Radar | Free | Free | ★★★★☆ |
| OpenSky Network | Free | Free | ★★★★☆ |
| IODA | Free | Free | ★★★★☆ |
| BGPStream / RIPE RIS Live | Free | Free | ★★★☆☆ |
| GeoConfirmed | Free | Free | ★★★★☆ |
| OONI | Free | Free | ★★★☆☆ |
| Global Fishing Watch | Free | Free | ★★★★☆ |
| AlienVault OTX | Free | Free | ★★★☆☆ |
| crt.sh / Certstream | Free | Free | ★★☆☆☆ |
| Copernicus/Sentinel | Free | Free | ★★★☆☆ |
| GreyNoise Community | Free | Free | ★★★☆☆ |
| **ADS-B Exchange Hobbyist** | **~$10** | **~$120** | **★★★★★** |
| **Total recommended** | **~$10** | **~$120** | |
