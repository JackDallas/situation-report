# Paid API Research for Situation Report

**Date**: 2026-03-04
**Purpose**: Evaluate paid data APIs for intelligence value-per-dollar to fill gaps in current OSINT coverage.

---

## 1. Current Coverage Matrix

### Active Sources (26 registered, all healthy or connecting)

| Source | Source Type | Status | 24h Events | Update Freq | Notes |
|--------|-----------|--------|-----------|-------------|-------|
| AIS (aisstream.io) | Maritime | Healthy (streaming) | 806,596 | Real-time WebSocket | 17,053 vessels tracked |
| AirplanesLive | Aviation | Healthy | 149,045 | 120s poll | Military + regional focus |
| adsb.fi | Aviation | Healthy | 147,701 | 120s poll | Failover source |
| adsb.lol | Aviation | Healthy | 137,422 | 120s poll | Failover source |
| BGP (RIPE RIS) | Cyber | Healthy | 91,115 | ~5min poll | Route anomalies |
| OpenSky | Aviation | Healthy | 21,659 | ~60s poll | OAuth2 authenticated |
| FIRMS (NASA) | Satellite/Thermal | Healthy | 11,216 | ~15min poll | Fire/thermal hotspots |
| Shodan (3 sources) | Cyber/Infra | Healthy | 5,768 | Mixed | Edu plan, limited |
| RSS News | News | Healthy | 4,281 | ~5min poll | Reuters, BBC, AJ, etc. |
| Telegram | OSINT | Connecting | 688 | Streaming | Direct channel monitoring |
| OONI | Cyber/Censorship | Healthy | 317 | ~60min poll | Internet censorship |
| NOTAM | Aviation/Airspace | Healthy | 236 | ~15min poll | Airspace restrictions |
| Cloudflare Radar | Cyber | Healthy | 66 | ~15min poll | Internet traffic anomalies |
| OTX (AlienVault) | Cyber/Threat Intel | Healthy | 39 | ~60min poll | Threat indicators |
| GeoConfirmed | Conflict/OSINT | Healthy | 1 | ~30min poll | Geolocated events |
| USGS | Seismic | Healthy | 0 (1 total) | ~15min poll | Earthquake data |
| GDELT | Conflict/News | Healthy | 0 (recent) | ~15min poll | Returning empty recently |
| GDELT Geo | News/Geo | Healthy | 0 (recent) | ~15min poll | No data flowing |
| Nuclear | Nuclear/Radiological | Healthy | 0 | ~60min poll | Rarely fires |
| IODA | Cyber/Outages | Healthy | 0 | ~15min poll | Internet outage detection |
| Cloudflare BGP | Cyber/BGP | Healthy | 0 | ~15min poll | BGP hijack detection |
| GPSJam | GPS/Navigation | Healthy | 0 | ~6hr poll | GPS interference zones |
| GFW | Maritime | Healthy | 0 | ~15min poll | Fishing vessel monitoring |
| CertStream | Cyber/Certificates | Connecting | 0 (in events) | Streaming | CT log monitoring |
| Shodan Stream | Cyber/Infra | Connecting | (counted in shodan) | Streaming | Real-time alert stream |

### Disabled Sources

| Source | Reason | Impact |
|--------|--------|--------|
| ACLED | Requires paid Research tier (institutional email) | **Missing ground-truth conflict data** |
| GreyNoise | Enterprise-only API | Missing internet noise classification |

### Data Volume Summary (last 24 hours)
- **Total events**: ~1.38 million
- **Position tracking**: ~1.26M (91% -- AIS + flight positions)
- **Cyber/BGP**: ~97K
- **Satellite/Thermal**: ~11K
- **News/OSINT**: ~5K
- **High-signal events** (conflict, news, geo, censorship, threat intel): ~5.3K

---

## 2. Gap Analysis

### Intelligence Questions We Cannot Answer Today

| Gap | Description | Impact |
|-----|-------------|--------|
| **Ground-truth conflict data** | No structured, coded conflict event data (ACLED disabled). GeoConfirmed has ~1 event/day, GDELT returning empty. | Cannot reliably correlate conflict events with other signals |
| **Vessel identity/ownership** | AIS gives position + basic static data (MMSI, name, type). No ownership, flag history, port call history, sanctions screening | Cannot answer "who owns this ship?" or "has it been sanctioned?" |
| **Aircraft identity enrichment** | ADS-B gives hex/callsign/type code. No operator database, route history, ownership | Cannot answer "what unit does this aircraft belong to?" or "where has it been?" |
| **Social media beyond Telegram** | No Twitter/X, no Reddit, no Bluesky. Telegram only covers channels we subscribe to | Missing the largest OSINT signal source for breaking events |
| **Satellite imagery** | FIRMS gives thermal anomalies only. No optical/SAR imagery for change detection | Cannot verify ground activity, detect construction, troop movements |
| **Weather context** | No weather data | Cannot correlate unusual flight patterns with weather vs. operational intent |
| **Sanctions/watchlist screening** | No screening against OFAC, EU, UN sanctions lists | Cannot flag sanctioned entities in vessel/aircraft tracking |
| **Military order of battle** | No structured military unit database | Cannot enrich military flights/vessels with unit/base information |
| **Dark web monitoring** | No dark web source | Missing early indicators of planned operations, leaked data |
| **IP geolocation enrichment** | Shodan banners have IPs but limited geo context for cyber events | BGP/Shodan events lack precise geolocation |

---

## 3. API Comparison by Category

### 3.1 Conflict Data

#### ACLED (Armed Conflict Location & Event Data)

| Aspect | Details |
|--------|---------|
| **What it provides** | Coded conflict events: battles, violence against civilians, protests, riots, strategic developments. Actor names, fatalities, precise geolocation, event classification. 1-week lag. |
| **Current status** | Source code written and tested. OAuth2 implemented. **Blocked by access tier**: requires "Research" level (institutional email) or higher. Personal email gets "Open" tier = aggregated data only, no API. |
| **Pricing** | **Not publicly listed** -- contact access@acleddata.com. Research tier requires institutional email registration. Partner/Enterprise tiers via negotiation. Some reports suggest Enterprise starts ~$10K-25K/year. |
| **Update frequency** | Weekly data releases, API updated accordingly |
| **Integration effort** | **Already built** -- `acled.rs` implements full OAuth2 + pagination. Just needs valid credentials. |
| **Intel value** | **CRITICAL** -- fills the single largest gap. Ground-truth conflict data with actor names, fatalities, and precise geo. Would immediately feed correlation rules (confirmed_strike, conflict_thermal). |
| **Recommendation** | **Priority 1**. Contact ACLED at access@acleddata.com. Explain Situation Report as a research project. If institutional email available, register with it for Research tier (likely free). Otherwise, explore Partner tier pricing. |

#### Alternative: Uppsala Conflict Data Program (UCDP)

| Aspect | Details |
|--------|---------|
| **What it provides** | Georeferenced event data (GED) on organized violence. Academic-grade, conservative coding. |
| **Pricing** | **Free** -- fully open access API at https://ucdp.uu.se/apidocs/ |
| **Update frequency** | Annual releases + candidate events mid-year |
| **Integration effort** | Medium -- new source implementation needed, REST API, JSON response |
| **Intel value** | Medium -- less timely than ACLED (annual vs weekly), but free and high quality |
| **Recommendation** | **Quick win** -- implement as supplement to ACLED. Free, no auth required. |

---

### 3.2 Maritime Enrichment

#### MarineTraffic API

| Aspect | Details |
|--------|---------|
| **What it provides** | Vessel details (ownership, flag, class, dimensions), port call history, voyage data, historical positions, satellite AIS, ETA predictions |
| **Pricing** | Web plans: Basic $10/mo, Standard $30/mo, Premium $100/mo. **API pricing is separate and enterprise-level** -- requires contacting sales. Historical reports suggest API starts ~$300-500/mo for basic vessel lookup. Full API suite $1,000-5,000/mo depending on endpoints and volume. |
| **Update frequency** | Real-time for tracking, vessel details updated as reported |
| **Integration effort** | Medium -- REST API, API key auth, straightforward JSON |
| **Intel value** | HIGH for vessel identity enrichment. We already have AIS position data (806K events/day). MarineTraffic adds the "who" and "why" layer: ownership chains, beneficial owners, flag state, sanctions flags. |
| **Recommendation** | **Priority 3**. Useful but expensive. Consider Datalastic as cheaper alternative. |

#### Datalastic

| Aspect | Details |
|--------|---------|
| **What it provides** | Ship tracking, vessel specs, ownership, port data, historical tracking, route tracking |
| **Pricing** | Starter: EUR 199/mo (20K credits), Experimenter: EUR 569/mo (80K credits), Developer Pro+: EUR 679/mo (unlimited). Add-ons (ownership, inspections, casualties): EUR 599/mo. |
| **Update frequency** | Real-time tracking, vessel data updated regularly |
| **Integration effort** | Medium -- REST API with credit-based billing |
| **Intel value** | Medium-High -- cheaper than MarineTraffic, includes ownership data |
| **Recommendation** | **Good alternative to MarineTraffic** if budget is constrained. Starter tier sufficient for enrichment lookups. |

#### Spire Maritime

| Aspect | Details |
|--------|---------|
| **What it provides** | Satellite AIS (open ocean coverage), 600K+ vessels/day, sub-1-minute latency. Historical data back to 2010. Vessel detection in congested waters. |
| **Pricing** | **Contact sales** -- enterprise-oriented. Reports suggest $2K-10K/mo depending on coverage area and data depth. Note: Spire sold its maritime business unit in 2025, future unclear. |
| **Update frequency** | Real-time satellite AIS |
| **Intel value** | Medium -- we already have good AIS coverage from aisstream.io (terrestrial). Satellite AIS adds open-ocean gaps but at high cost. |
| **Recommendation** | **Low priority** -- our aisstream.io coverage is strong. Only valuable if we need deep ocean tracking (e.g., sanctions evasion, dark fleet monitoring). |

---

### 3.3 Aviation Enrichment

#### FlightAware AeroAPI (v4)

| Aspect | Details |
|--------|---------|
| **What it provides** | Flight status, route info, aircraft details, operator info, historical flights back to 2011, ETA predictions (Foresight), flight alerts. 60+ endpoints. |
| **Pricing** | Usage-based (v4): ~$0.01-0.10 per API call depending on endpoint. Free tier gives limited queries. Subscription tiers: Personal (free, limited), Standard (~$1/query for most endpoints), Premium ($0.001-0.005/query with volume commitment). Enterprise Firehose: custom pricing ($5K+/mo). |
| **Update frequency** | Real-time for active flights, historical queries on demand |
| **Integration effort** | Medium -- REST API, API key auth, well-documented OpenAPI 3.1 spec |
| **Intel value** | HIGH for enrichment. We track ~450K flight positions/day but only know hex/callsign/type. FlightAware adds: operator name, route (origin/destination), aircraft registration details, flight history. Critical for answering "where is this military transport going?" |
| **Recommendation** | **Priority 2**. Start with free tier for testing, upgrade to pay-per-query for enrichment of high-value military flights only. At ~8K military flight events/day, enriching only flagged aircraft at $0.01/call = ~$2.40/day = **$72/month**. Very good value. |

#### Spire Aviation

| Aspect | Details |
|--------|---------|
| **What it provides** | Satellite + terrestrial ADS-B, aircraft/airline fleet data, flight schedule enrichment, emissions, holding patterns, go-arounds |
| **Pricing** | Essential/Standard/Premium tiers -- contact sales. Reports suggest $1K-5K/mo. |
| **Update frequency** | Real-time tracking with satellite + terrestrial |
| **Intel value** | Medium -- overlaps with our existing ADS-B sources. Adds satellite coverage for oceanic/remote areas. |
| **Recommendation** | **Low priority** -- we have 3 ADS-B aggregators already providing good coverage. Only useful for remote/oceanic gaps. |

---

### 3.4 Social Media Monitoring

#### Twitter/X API

| Aspect | Details |
|--------|---------|
| **What it provides** | Post search, user data, real-time stream, full archive access at higher tiers |
| **Pricing** | Free: nearly useless (~1 req/15min). Basic: **$200/mo** (15K reads/mo, 7-day search). Pro: **$5,000/mo** (advanced search, higher limits). Enterprise: **$42,000+/mo** (full archive, firehose). |
| **Update frequency** | Real-time with streaming API (Pro+), near-real-time with search |
| **Integration effort** | Medium-High -- OAuth2, complex rate limiting, need to manage search queries |
| **Intel value** | HIGH in theory (Twitter is the #1 OSINT platform for breaking events), but **terrible value at current pricing**. Basic tier at $200/mo gives only 15K reads -- not enough for continuous monitoring. Pro at $5K/mo is cost-prohibitive. |
| **Recommendation** | **Skip official API**. Instead, consider third-party providers (TwitterAPI.io at $0.15/1K tweets) or focus on Bluesky as a free alternative. |

#### Third-Party Twitter/X Access (TwitterAPI.io, SocialData)

| Aspect | Details |
|--------|---------|
| **What it provides** | Read-only access to Twitter/X data, full archive search, pay-as-you-go |
| **Pricing** | TwitterAPI.io: **$0.15 per 1,000 tweets** (pay-as-you-go, no subscription). SocialData: similar pricing. |
| **Update frequency** | Near-real-time (not true streaming) |
| **Integration effort** | Low-Medium -- REST API, simple auth |
| **Intel value** | High -- gets Twitter data at ~96% cost reduction vs official API |
| **Recommendation** | **Priority 4**. Good value if Twitter monitoring is needed. At $0.15/1K tweets, monitoring 10K tweets/day = **$45/month**. No subscription commitment. |

#### Bluesky AT Protocol / Firehose

| Aspect | Details |
|--------|---------|
| **What it provides** | Full public firehose of all Bluesky posts, likes, reposts. Jetstream for filtered access. Growing OSINT community presence. |
| **Pricing** | **FREE** -- fully open protocol, no API key needed for public firehose |
| **Update frequency** | Real-time WebSocket streaming |
| **Integration effort** | Medium -- WebSocket (similar to our AIS implementation), CBOR or JSON parsing. Jetstream recommended for filtered access. |
| **Intel value** | Medium and growing -- Bluesky has a strong OSINT community. Many journalists and analysts migrated from Twitter. Growing but still much smaller than Twitter. |
| **Recommendation** | **Quick win**. Free, real-time, well-documented protocol. Implement as a streaming source similar to AIS. Monitor OSINT-relevant accounts/keywords. |

#### Reddit

| Aspect | Details |
|--------|---------|
| **What it provides** | Post and comment data from subreddits. Strong for niche community signals (r/worldnews, r/ukraine, r/cybersecurity, etc.) |
| **Pricing** | Free tier: 100 queries/min (non-commercial). Commercial: $0.24/1K API calls. |
| **Update frequency** | Near-real-time polling |
| **Integration effort** | Medium -- OAuth2, REST API |
| **Intel value** | Low-Medium for OSINT. Reddit is slower than Twitter for breaking news but good for community analysis and sentiment. |
| **Recommendation** | **Low priority**. Free tier sufficient for basic monitoring. Not a primary OSINT signal source. |

---

### 3.5 Cyber / Threat Intelligence

#### Censys

| Aspect | Details |
|--------|---------|
| **What it provides** | Internet-wide scanning (like Shodan), host/service discovery, certificate transparency, ASM. Better coverage of IPv6 and cloud assets than Shodan. |
| **Pricing** | Free: 250 queries/mo. Solo: **$25/mo** (2,500 queries). Team/Enterprise: contact sales. |
| **Update frequency** | Continuous scanning, data refreshed daily |
| **Integration effort** | Low -- REST API, API key auth. Could complement or replace Shodan sources. |
| **Intel value** | Medium -- complements Shodan. Better for cloud/IPv6 scanning. Our Shodan edu plan gives 100 query credits/mo + 100 scan credits/mo. Censys Solo plan at $25/mo would triple our scan capability. |
| **Recommendation** | **Quick win at $25/month**. Add as supplementary scan source alongside Shodan. Particularly valuable for ICS/critical infrastructure discovery. |

#### GreyNoise

| Aspect | Details |
|--------|---------|
| **What it provides** | Internet background noise classification. Identifies benign scanners, known bots, mass exploitation campaigns. Reduces false positives by filtering "noise" from real threats. |
| **Pricing** | Community (free): limited lookups, basic metadata. Block: **$999/mo** or **$9,999/year**. Full Platform: enterprise pricing (contact sales). |
| **Update frequency** | Real-time classification |
| **Integration effort** | Low -- REST API, straightforward IP lookup |
| **Intel value** | Medium -- valuable for reducing noise in our Shodan/BGP/OTX data streams. But at $999/mo, poor value for our use case. |
| **Recommendation** | **Skip for now**. The community API (free) provides enough for spot-checking. Our OTX + Shodan combination already covers most threat intel needs. $999/mo is overkill for our scale. |

#### Recorded Future

| Aspect | Details |
|--------|---------|
| **What it provides** | AI-driven threat intelligence: finished intelligence reports, IOC enrichment, dark web monitoring, vulnerability intelligence, MITRE ATT&CK mapping. |
| **Pricing** | Median buyer pays **~$69K/year** ($5,750/mo). Range: $27K-$125K/year. |
| **Update frequency** | Real-time intelligence feeds |
| **Integration effort** | Medium -- REST API, 5,000 API calls/day included |
| **Intel value** | Very high for enterprise SOCs. **Massive overkill for Situation Report**. We need conflict/geopolitical intelligence, not enterprise SOC threat feeds. |
| **Recommendation** | **Skip**. Far too expensive and focused on enterprise cybersecurity rather than geopolitical OSINT. |

#### AbuseIPDB

| Aspect | Details |
|--------|---------|
| **What it provides** | Community-driven IP reputation database. Check if IPs are reported for malicious activity. |
| **Pricing** | Free: 1,000 lookups/day. Basic: **$25/mo** (10K/day). Premium: **$99/mo** (50K/day). |
| **Update frequency** | Real-time community reports |
| **Integration effort** | Very low -- single API endpoint for IP lookup |
| **Intel value** | Low-Medium -- useful for enriching Shodan/BGP IP addresses with reputation data. Free tier (1K/day) likely sufficient. |
| **Recommendation** | **Quick win (free)**. Use free tier to enrich high-severity cyber events. No new source needed -- add enrichment to existing pipeline. |

#### VirusTotal

| Aspect | Details |
|--------|---------|
| **What it provides** | Multi-AV scanning, URL/domain/IP reputation, file analysis, threat graphs |
| **Pricing** | Free: ~500 lookups/day. Premium: ~$833/mo. Enterprise: $10K+/year. |
| **Update frequency** | Real-time |
| **Integration effort** | Low -- REST API |
| **Intel value** | Low for our use case -- VirusTotal is file/malware focused. We don't analyze files. |
| **Recommendation** | **Skip**. Not relevant to our geopolitical OSINT mission. |

---

### 3.6 Real-Time Event Detection

#### Dataminr

| Aspect | Details |
|--------|---------|
| **What it provides** | AI-driven real-time event and risk detection from public data (social media, news, sensors). Breaking event alerts often 30-60 minutes before wire services. ReGenAI for continuously updated event briefs. |
| **Pricing** | Median buyer pays **~$20K/year**. Range: $17.5K-$63.75K/year. |
| **Update frequency** | Real-time (seconds to minutes) |
| **Integration effort** | Medium -- API integration, webhook alerts |
| **Intel value** | Very high -- essentially what we're building ourselves with the pipeline. Dataminr is the gold standard for real-time event detection. |
| **Recommendation** | **Skip**. We are building our own event detection pipeline. Dataminr's value proposition overlaps with Situation Report's core purpose. At $20K+/year, it's also expensive. However, worth monitoring as a benchmark for our pipeline's performance. |

---

### 3.7 Satellite Imagery

#### Planet (PlanetScope + SkySat)

| Aspect | Details |
|--------|---------|
| **What it provides** | Daily global coverage at 3-5m resolution (PlanetScope), on-demand sub-meter imagery (SkySat), change detection analytics, vessel detection |
| **Pricing** | Hectares Under Management model for PlanetScope. ~EUR 100 per quota package (100 ha/year). SkySat tasking: ~$8.50/km2. Enterprise: custom pricing, typically $10K-50K+/year depending on area. |
| **Update frequency** | Daily revisit (PlanetScope), on-demand tasking (SkySat) |
| **Integration effort** | High -- requires image processing pipeline, change detection algorithms |
| **Intel value** | Very high in theory -- satellite imagery is the missing capability for verifying ground events. But requires significant infrastructure investment beyond just API integration. |
| **Recommendation** | **Future phase**. The integration effort and cost are both high. Revisit once core pipeline is mature. For now, rely on FIRMS thermal data and GeoConfirmed verified imagery. |

#### Sentinel Hub (Copernicus / ESA)

| Aspect | Details |
|--------|---------|
| **What it provides** | Access to Sentinel-1 (SAR), Sentinel-2 (optical), Landsat imagery. 10m resolution Sentinel-2, 5-day revisit. Change detection scripts. |
| **Pricing** | **Free tier**: 30-day trial. Paid: based on "Processing Units" -- EUR 1 per PU, with tiered discounts. Sentinel data itself is free; you pay for processing/API access. Typical: EUR 300-1,000/month for moderate use. |
| **Update frequency** | 5-day revisit (Sentinel-2), 6-day (Sentinel-1 SAR) |
| **Integration effort** | High -- OGC APIs (WMS/WCS), custom scripting for change detection, significant data processing |
| **Intel value** | Medium-High -- free imagery data, but 10m resolution limits utility for military targets. SAR is valuable for all-weather, day/night monitoring. |
| **Recommendation** | **Future phase**. Consider for v2 when we add imagery analysis capability. The free data access is attractive but the processing pipeline is a significant project. |

---

### 3.8 IP Geolocation Enrichment

#### IPinfo

| Aspect | Details |
|--------|---------|
| **What it provides** | IP geolocation (city/region/country), ASN/ISP, VPN/proxy detection, hosted domain data, abuse contacts |
| **Pricing** | Free: 50K requests/mo (country-level only since 2025). Basic: **$49/mo** (city-level, 500K/mo). Standard: **$99/mo** (1M/mo + ASN). Business: $249/mo. |
| **Update frequency** | Database updated continuously |
| **Integration effort** | Very low -- single REST endpoint, API key auth |
| **Intel value** | Medium -- enriches Shodan/BGP/OTX events with geolocation. Helps answer "where is this scanning/attack coming from?" |
| **Recommendation** | Consider at $49/mo if IP enrichment is a priority. Alternatively, use MaxMind's free GeoLite2 database (download, no API calls needed). |

#### MaxMind GeoLite2

| Aspect | Details |
|--------|---------|
| **What it provides** | IP geolocation (city/country), ASN lookup. Free downloadable database. |
| **Pricing** | **Free** (GeoLite2). Paid GeoIP2: starts at $30/mo for more accuracy. |
| **Update frequency** | Weekly database updates |
| **Integration effort** | Low -- download MMDB file, use in-process with mmdb reader crate. No API calls needed. |
| **Intel value** | Medium -- same as IPinfo but free and faster (local lookup, no network round-trip) |
| **Recommendation** | **Quick win (free)**. Download GeoLite2-City database, integrate with Rust mmdb reader for zero-cost IP geolocation enrichment on all cyber events. |

---

### 3.9 Entity Enrichment

#### Wikidata SPARQL

| Aspect | Details |
|--------|---------|
| **What it provides** | 100M+ entities with structured data. Entity types, relationships, aliases, geographic coordinates, Wikipedia links. Covers political figures, military organizations, cities, etc. |
| **Pricing** | **Free** -- public SPARQL endpoint at query.wikidata.org |
| **Update frequency** | Near-real-time (community-edited) |
| **Integration effort** | Medium -- SPARQL queries, rate limiting (~60 req/min). Already referenced in entity resolver code. |
| **Intel value** | Medium-High -- enriches our entity graph with structured data. Adds aliases (e.g., "Houthis" = "Ansar Allah"), hierarchies, and geographic context. |
| **Recommendation** | **Quick win (free)**. Already partially integrated in entity resolver. Expand to bulk entity enrichment during pipeline idle time. |

#### OpenCorporates

| Aspect | Details |
|--------|---------|
| **What it provides** | Legal entity data for 200M+ companies worldwide. Registration details, officers, ownership chains, jurisdictions. |
| **Pricing** | **Contact sales** for API access. Reports suggest $500-2,000/mo for commercial API. Free website search available. |
| **Update frequency** | Varies by jurisdiction, mostly weekly-monthly |
| **Integration effort** | Medium -- REST API |
| **Intel value** | Medium -- useful for vessel/aircraft ownership resolution. "Who owns this ship?" requires tracing through shell companies. |
| **Recommendation** | **Future phase**. Only valuable once we have a specific need for corporate ownership tracing (e.g., sanctions investigation). |

#### SIPRI Databases

| Aspect | Details |
|--------|---------|
| **What it provides** | Arms transfers between countries (since 1950), military expenditure data, arms industry companies. |
| **Pricing** | **Free** -- public web interface. Unofficial Python API wrapper available (benryan58/sipri_arms). |
| **Update frequency** | Annual updates |
| **Integration effort** | Medium -- unofficial API, web scraping. Could bulk-import static data. |
| **Intel value** | Medium -- background context for understanding military capabilities. Not real-time operational intelligence. |
| **Recommendation** | **Low priority**. Useful as static reference data for enrichment (e.g., "Country X recently purchased Y weapons system") but not a real-time source. |

---

### 3.10 Military / Defense Intelligence

#### Janes (IHS Markit)

| Aspect | Details |
|--------|---------|
| **What it provides** | The gold standard for open-source military intelligence. 89K+ equipment profiles, 37K+ military units with base locations, 29K+ geolocated installations, orders of battle for all militaries, 900K+ linked events, PMESII country reports. API-enabled knowledge graph. |
| **Pricing** | **Enterprise only** -- typically $50K-200K+/year depending on modules. No published pricing. |
| **Update frequency** | Continuous updates by 500+ analysts |
| **Integration effort** | Medium -- API available, knowledge graph format |
| **Intel value** | **Extremely high** -- answers "what military unit is at this base?", "what equipment does this country have?", "what are the known installations near this location?" |
| **Recommendation** | **Aspirational / Future phase**. Far too expensive for current scope. But worth understanding as the ceiling for military OSINT enrichment. Consider building our own military base/unit database from open sources as a cheaper alternative (we already have a military-bases.geojson). |

---

### 3.11 Sanctions / Watchlist Screening

#### OpenSanctions

| Aspect | Details |
|--------|---------|
| **What it provides** | Aggregated sanctions lists (OFAC, EU, UN, etc.), PEP lists, debarment lists. Entity matching API. |
| **Pricing** | **Free for non-commercial use**. Commercial: EUR 500-2,000/mo. Self-hosted: free (open-source dataset). |
| **Update frequency** | Daily updates |
| **Integration effort** | Low-Medium -- REST API for matching, or download full dataset and match locally |
| **Intel value** | HIGH -- enables automatic flagging of sanctioned vessels/entities in AIS tracking. Answers "is this vessel/entity under sanctions?" |
| **Recommendation** | **Quick win (free for research)**. Download the dataset and integrate local matching against AIS vessel names/MMSI and entity graph. |

---

## 4. Recommended Priority List

### Tier 1: Implement Now (Free or very cheap, high value)

| # | API/Source | Monthly Cost | Intel Value | Effort |
|---|-----------|-------------|-------------|--------|
| 1 | **ACLED** (if Research tier obtainable) | $0 (institutional email) | Critical | **Already built** |
| 2 | **Bluesky Firehose** | $0 | Medium-High | Medium (WebSocket, like AIS) |
| 3 | **MaxMind GeoLite2** | $0 | Medium | Low (download DB) |
| 4 | **UCDP API** | $0 | Medium | Medium (new source) |
| 5 | **OpenSanctions** (self-hosted) | $0 | High | Medium (dataset download + matcher) |
| 6 | **AbuseIPDB** (free tier) | $0 | Low-Medium | Very Low (enrichment lookup) |
| 7 | **Wikidata SPARQL** (expanded) | $0 | Medium | Low (already partial) |

**Estimated monthly cost: $0**
**Total new intel capabilities: 7**

### Tier 2: High Value / Low Cost

| # | API/Source | Monthly Cost | Intel Value | Effort |
|---|-----------|-------------|-------------|--------|
| 8 | **Censys Solo** | $25/mo | Medium | Low |
| 9 | **FlightAware AeroAPI** (enrichment) | ~$72/mo | High | Medium |
| 10 | **TwitterAPI.io** (third-party X) | ~$45/mo | High | Medium |

**Estimated monthly cost: ~$142**
**Total new intel capabilities: 3**

### Tier 3: Medium Value / Medium Cost

| # | API/Source | Monthly Cost | Intel Value | Effort |
|---|-----------|-------------|-------------|--------|
| 11 | **Datalastic** (vessel enrichment) | EUR 199/mo (~$215) | Medium-High | Medium |
| 12 | **IPinfo Basic** | $49/mo | Medium | Very Low |
| 13 | **ACLED** (if paid tier required) | ~$200-500/mo (est.) | Critical | Already built |

**Estimated monthly cost: ~$264-564**
**Total new intel capabilities: 2-3**

### Tier 4: Future Phase (High cost, requires infrastructure)

| # | API/Source | Monthly Cost | Intel Value | Effort |
|---|-----------|-------------|-------------|--------|
| 14 | Planet / Sentinel Hub (satellite imagery) | $500-5,000/mo | Very High | Very High |
| 15 | MarineTraffic API | $300-5,000/mo | High | Medium |
| 16 | Janes Defense Intelligence | $4K-17K/mo | Very High | Medium |
| 17 | Dataminr | $1.5K-5K/mo | High | Medium |
| 18 | Recorded Future | $2.3K-10K/mo | High | Medium |

### Not Recommended

| API | Reason |
|-----|--------|
| Twitter/X Official API | $200/mo for Basic is terrible value (15K reads). $5K/mo for Pro is cost-prohibitive. Use third-party instead. |
| GreyNoise Paid | $999/mo for noise classification is overkill. Free community API sufficient. |
| VirusTotal | File/malware focused, not relevant to geopolitical OSINT. |
| Spire Maritime/Aviation | Overlaps with existing free AIS/ADS-B sources at high cost. |
| Recorded Future | Enterprise SOC tool, not geopolitical OSINT. $69K/year median. |

---

## 5. Quick Wins (Free or Near-Free)

These can be implemented immediately with zero or minimal cost:

### 5.1 UCDP Conflict Data (Free)
- **URL**: https://ucdp.uu.se/apidocs/
- **What**: Georeferenced conflict event data, academic-grade
- **Implementation**: New polling source, similar to ACLED but simpler (no auth)
- **Value**: Fills conflict data gap until ACLED access is resolved

### 5.2 Bluesky Firehose (Free)
- **URL**: wss://jetstream2.us-east.bsky.network/subscribe
- **What**: Real-time public post stream from Bluesky social network
- **Implementation**: WebSocket streaming source, filter for OSINT keywords/accounts
- **Value**: Social media coverage with zero API cost, growing OSINT community

### 5.3 MaxMind GeoLite2 (Free)
- **URL**: https://dev.maxmind.com/geoip/geolite2-free-geolocation-data
- **What**: IP geolocation database, downloadable, weekly updates
- **Implementation**: Download MMDB, integrate with `maxminddb` Rust crate for in-process lookup
- **Value**: Enriches all cyber events with IP geolocation at zero API cost

### 5.4 OpenSanctions Dataset (Free for research)
- **URL**: https://www.opensanctions.org/docs/api/
- **What**: Aggregated global sanctions lists with entity matching
- **Implementation**: Download dataset, build local fuzzy matcher against entity graph + AIS vessel names
- **Value**: Automatic sanctions screening on tracked vessels and entities

### 5.5 AbuseIPDB Free Tier (Free)
- **URL**: https://www.abuseipdb.com/api
- **What**: IP reputation database, 1,000 lookups/day free
- **Implementation**: Enrichment lookup for IPs in Shodan/BGP events
- **Value**: Flag known-malicious IPs in cyber event stream

### 5.6 Wikidata Entity Enrichment (Free)
- **URL**: https://query.wikidata.org/sparql
- **What**: Structured entity data, aliases, relationships
- **Implementation**: Expand existing entity resolver to query Wikidata for entity details
- **Value**: Better entity resolution (aliases, name variants, geographic context)

### 5.7 Reddit Free Tier (Free)
- **URL**: https://www.reddit.com/dev/api/
- **What**: Subreddit monitoring (r/worldnews, r/ukraine, r/cybersecurity, etc.)
- **Implementation**: Simple polling source, 100 req/min free
- **Value**: Community intelligence signals, slower than Twitter but free

---

## 6. Integration Notes

### Effort Estimates

| Source | Lines of Code (est.) | Dependencies | Complexity | Timeline |
|--------|---------------------|--------------|------------|----------|
| UCDP | ~200 | None new | Simple REST polling | 1-2 hours |
| Bluesky Firehose | ~300 | `tokio-tungstenite` (already used) | WebSocket streaming | 3-4 hours |
| MaxMind GeoLite2 | ~150 | `maxminddb` crate | Local DB lookup | 2-3 hours |
| OpenSanctions | ~400 | JSON dataset parsing | Fuzzy matching | 4-6 hours |
| AbuseIPDB enrichment | ~100 | None new | HTTP lookup in pipeline | 1-2 hours |
| Censys source | ~250 | None new | REST polling | 2-3 hours |
| FlightAware enrichment | ~300 | None new | REST lookup in pipeline | 3-4 hours |
| TwitterAPI.io source | ~250 | None new | REST polling | 2-3 hours |
| Datalastic enrichment | ~250 | None new | REST lookup in pipeline | 2-3 hours |

### Architecture Notes

- **Enrichment sources** (AbuseIPDB, FlightAware, Datalastic, MaxMind, OpenSanctions) should be implemented as **pipeline enrichment steps**, not as independent polling sources. They enrich existing events rather than generating new ones.
- **New polling/streaming sources** (UCDP, Bluesky, Censys, TwitterAPI.io, Reddit) follow the existing `DataSource` trait pattern.
- **MaxMind GeoLite2** should be loaded at startup as an in-memory database, shared via `Arc<maxminddb::Reader>`. Zero-cost per lookup.
- **OpenSanctions** matching should run as a background task against new AIS entity names, caching results to avoid repeated lookups.

### Cost Summary by Phase

| Phase | Sources | Monthly Cost | Annual Cost |
|-------|---------|-------------|-------------|
| **Phase 1** (free quick wins) | UCDP, Bluesky, MaxMind, OpenSanctions, AbuseIPDB, Wikidata | **$0** | **$0** |
| **Phase 2** (cheap paid) | + Censys, FlightAware, TwitterAPI.io | **~$142** | **~$1,704** |
| **Phase 3** (enrichment) | + Datalastic, IPinfo | **~$406** | **~$4,872** |
| **Phase 4** (enterprise) | + Satellite imagery, MarineTraffic, Janes | **$5K-25K+** | **$60K-300K+** |

---

## 7. Summary of Recommendations

**Immediate action items:**

1. **Contact ACLED** (access@acleddata.com) about Research tier access -- this is the single highest-value data source we're missing, and the code is already written
2. **Implement UCDP** as a free conflict data source while ACLED access is pending
3. **Add Bluesky Firehose** as a free social media streaming source
4. **Download MaxMind GeoLite2** for zero-cost IP geolocation enrichment
5. **Download OpenSanctions dataset** for vessel/entity sanctions screening
6. **Add AbuseIPDB free tier** enrichment to cyber event pipeline

**Next month (budget ~$142/mo):**

7. **Subscribe to Censys Solo** ($25/mo) to complement Shodan
8. **Set up FlightAware AeroAPI** for military flight enrichment (~$72/mo)
9. **Add TwitterAPI.io** for Twitter/X monitoring (~$45/mo)

**Key insight**: The best intelligence value comes from **enriching data we already have** (AIS vessels, ADS-B flights, Shodan IPs) rather than adding entirely new data streams. The Phase 1 enrichment items (MaxMind, OpenSanctions, AbuseIPDB, Wikidata) are all free and add significant analytical depth to our existing 1.38M events/day.
