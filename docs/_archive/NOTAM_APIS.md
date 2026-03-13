# Every NOTAM API compared for conflict monitoring

**The FAA NOTAM system and a little-known free UK XML feed are the strongest foundations for an open-source OSINT airspace monitoring platform, while the ICAO and Eurocontrol services carry prohibitive licensing or cost barriers.** No single API covers all needs — global conflict-zone monitoring requires stitching together at least two or three sources. The good news: multiple free options exist, a handful of affordable commercial APIs fill the gaps, and the Rust ecosystem has the parsing primitives to build a solid ingestion pipeline. This report covers every major NOTAM data source available for programmatic access, with specific focus on conflict-relevant signals like GPS jamming, airspace closures, and military activity NOTAMs.

---

## The FAA system is free, well-documented, and the best starting point

The US FAA offers **three distinct free access methods** for NOTAM data, each serving different architectures:

**FAA NOTAM REST API** (`api.faa.gov`) is the simplest entry point. Registration requires creating an account at the FAA API portal, requesting access to the NOTAM API, and waiting for manual approval (typically days). Authentication uses a `client_id` and `client_secret` header pair — no OAuth complexity. The API returns **JSON with structured fields** including ICAO location codes, Q-codes, coordinates, classification (military/domestic/FDC), and full NOTAM text. Filtering supports location identifiers, NOTAM classification, feature type, and free-text search — meaning you can query for terms like "GPS UNRELIABLE" or "AIRSPACE CLOSED" directly. An example call looks like:

```
GET https://api.faa.gov/s/notam/api/v4/notams?locationDesignator=KJFK
Headers: client_id: <key>, client_secret: <secret>
```

Rate limits exist but specific thresholds are disclosed only after registration. Coverage is primarily the US National Airspace System, but international NOTAMs received by the FAA — including foreign flight prohibitions filed under designator **KZZZ** — are also available, making this partially useful for global monitoring.

**FAA SWIM/SCDS** (`scds.faa.gov`) is the real-time option. This is a JMS publish/subscribe stream via Solace messaging that pushes every NOTAM update as it happens, in **AIXM 5.1 XML** format. Setup is more involved: register at the SCDS portal, request an AIM FNS subscription, receive SFTP credentials for the FNS Initial Load (a bulk download of all active NOTAMs to bootstrap a local database), then connect a JMS client for streaming updates. The FAA provides an Apache 2.0-licensed reference implementation at `github.com/faa-swim/fns-client` that exposes local REST endpoints after setup. This architecture maps well to a self-hosted PostgreSQL/PostGIS deployment on Hetzner — ingest the FIL dump, then apply real-time deltas from JMS. The critical gotcha: the reference client requires receiving **all messages unfiltered** or its missed-message detection breaks.

**NASA DIP NOTAMs API** (`dip.amesaero.nasa.gov`), published in April 2025, redistributes FAA SWIM data as a clean REST/JSON API with value-added geospatial and temporal extraction. It is free, publicly accessible, and the most developer-friendly option for getting structured US NOTAM data quickly. Coverage remains US-only.

For terms of service, FAA data is largely public domain under US federal law. The key restriction: **you may not characterize redistributed data as "FAA data"** — attribution must be indirect. No explicit prohibition exists on OSINT, conflict monitoring, or open-source use. TFR geometry is a known pain point — TFR NOTAMs from the API often lack geometry data, requiring a separate pull from `tfr.faa.gov` to get shapefiles.

---

## ICAO offers global reach but at punishing cost

The ICAO API Data Service provides the only official global NOTAM API, but its pricing model makes continuous monitoring impractical for most projects. Registration at `applications.icao.int/dataservices/default.aspx` requires a professional email and organizational affiliation. An API key arrives automatically and comes preloaded with **100 lifetime API calls** — not per month, not per day, lifetime. Authentication is a simple `api_key` query parameter.

The service runs on AWS API Gateway and offers two NOTAM endpoints. The **realtime endpoint** (`realtime-notams`) queries the FAA Defense Internet NOTAM Service (DINS) live, returning data as fresh as the source. The **stored endpoint** (`notams-list`) draws from a batch import updated every **3–12 hours** (ICAO's own documentation contradicts itself). Both return JSON or CSV with fields including Q-code breakdowns, AI-assessed criticality scores (0–4 via ICAO's NORM system), state codes, and full NOTAM text. Filtering supports ICAO location codes, Q-codes, keywords via a `qstring` parameter, and state codes — but **no coordinate-based geographic queries** and **no date range filtering**, which are significant gaps for conflict-zone monitoring.

The cost structure is the dealbreaker. After exhausting 100 free calls, booster packs cost **$525 for 2,000 calls** (~$0.26/call) or **$1,575 for 10,000 calls** (~$0.16/call). Monitoring 50 airports hourly burns 1,200 calls per day — roughly **$200–300 daily**. Terms of service prohibit redistribution and derivative works, requiring a custom licensing agreement (contact `ICAOAPI@icao.int`) for any open-source platform that would expose the data. The underlying NOTAM data comes from the same FAA DINS system accessible through other channels, making the ICAO API a convenience wrapper with restrictive terms rather than a unique data source.

---

## Eurocontrol's walled garden blocks most non-aviation users

Eurocontrol operates three systems relevant to NOTAM data, all protected by significant access barriers:

**NM B2B Web Services** is the most technically capable but most restrictive. Access requires submitting a service request, signing multiple agreements, designating an organizational single point of contact, obtaining client certificates (PKI/X.509 with mutual TLS), and passing a 10-day operational validation period. **Eligibility is limited to aviation operational entities** — ANSPs, airlines, airports, ground handlers, and flight plan service providers. Open-source research projects are explicitly outside scope. Even if access were granted, the NM B2B does not serve raw NOTAMs — it provides the Network Manager's operational interpretation of airspace reflecting NOTAM implementations, delivered as AIXM 5.1.1 XML over SOAP. Terms strictly prohibit redistribution and limit use to "operational ATFCM purposes and ATM-related studies."

**EAD (European AIS Database)** is the authoritative source of European NOTAM data, serving 56 states and ~200 data users worldwide. It operates in tiers: **EAD Basic** is free and open to the public but provides only web-based browsing with no API access and a limited data set explicitly marked "not for operational purposes." The programmatic tier, **MyEAD**, requires signing a formal Data User Agreement, obtaining an AIMSL B2B license and EACP certificate, and developing a SOAP/XML client against Eurocontrol's APIs. Pricing depends on client type — entities from member states pay nothing, research organizations pay service charges, and commercial users pay service charges plus royalties. Reselling EAD data is prohibited across all tiers.

**The Digital NOTAM Subscription and Request Service** is the most modern offering, delivering structured AIXM 5.1.1 data via REST subscription management and AMQP 1.0 distribution. It supports filtering by event scenario, NOTAM series, publisher, and specific aerodromes or airspaces. However, it requires the same MyEAD-level access credentials and is currently limited in scope (initially 18 airports under EU Implementing Regulation 2021/116).

For an OSINT conflict monitoring platform, Eurocontrol's systems are effectively inaccessible. The eligibility requirements, certificate complexity, formal agreements, and redistribution prohibitions create insurmountable barriers for an open-source project.

---

## The commercial and open-source alternatives that actually work

Beyond the big three institutional providers, several commercial APIs and open-source resources fill critical gaps:

**Notamify** (`notamify.com`) stands out as the most practical commercial option for this use case. It is a **European company** offering a REST API with bearer-token authentication, JSON responses with 42 standardized NOTAM categories, and — uniquely — **a historical archive spanning 2 years**, which is valuable for conflict trend analysis. Pricing starts at $24.90/month for API access plus credit packages ($0.20–0.30 per page of results). They have released open-source AIXM Python bindings. The AI-powered categorization can help automatically flag conflict-relevant NOTAMs.

**Aviation Edge** (`aviation-edge.com`) provides a straightforward global NOTAM API with instant API key registration. Query by ICAO/IATA code, filter by date range, get JSON back. Pricing has recently increased — current rates appear to be **$99–299/month** for the developer tier (30,000 calls/month). Global coverage, simple integration, but no FIR-level queries.

**Laminar Data** (now part of Cirium/Moody's) offers the highest-quality data with **GeoJSON responses including geometry polygons** — ideal for mapping conflict zones. It supports FIR-level queries, which is exactly what conflict monitoring needs. However, it targets enterprise customers with unlisted pricing that is likely cost-prohibitive.

**The NATS UK contingency XML feed** is a critical discovery. Available at the NATS pre-flight information bulletins page, this feed provides **all UK NOTAMs valid at generation time plus the next 7 days** in XML format, updated hourly, with **no authentication required**. An open-source project (`github.com/Jonty/uk-notam-archive`) already archives this feed hourly. The UK frequently issues GPS jamming exercise NOTAMs, making this directly relevant to conflict-adjacent monitoring. This same pattern — scraping national AIS contingency feeds — may work for other European countries.

| Source | Auth | Format | Coverage | Free tier | Best for |
|--------|------|--------|----------|-----------|----------|
| FAA REST API | API key pair | JSON | US + some intl | Fully free | US NOTAMs, military classification |
| FAA SWIM/SCDS | JMS + SFTP | AIXM XML | US | Fully free | Real-time streaming, self-hosted DB |
| NASA DIP | None (public) | JSON | US | Fully free | Quick structured US data |
| ICAO API | API key param | JSON/CSV | Global | 100 calls only | Spot-checks, not continuous |
| Eurocontrol EAD | Client cert | AIXM XML | Europe (56 states) | Web-only basic | Inaccessible for this use case |
| Notamify | Bearer token | JSON | Global | 7-day trial | Affordable global monitoring |
| Aviation Edge | API key param | JSON | Global | Limited | Simple global queries |
| Laminar/Cirium | API key | GeoJSON/AIXM | Global | None | Enterprise with geometry needs |
| NATS UK feed | None | XML | UK | Fully free | UK GPS jamming, scraping model |

---

## Building the Rust ingestion pipeline

No dedicated NOTAM crate exists on crates.io, so you will need to build parsing infrastructure. The **`nom`** parser combinator library (353M+ downloads) is the ideal choice for parsing ICAO NOTAM text format — it is zero-copy, byte-oriented, and extremely fast. For AIXM 5.1 XML from SWIM or EAD feeds, **`quick-xml`** provides high-performance XML parsing. Three open-source NOTAM parsers serve as excellent reference implementations to port: `svoop/notam` (Ruby, MIT license, most complete ICAO parser), `slavak/PyNotam` (Python, clean field extraction), and `dfelix/notam-decoder` (JavaScript, based on ICAO AIS Manual).

For conflict-relevant signal detection, the **WorldMonitor** project (`github.com/koala73/worldmonitor`) demonstrates the exact pattern needed: Q-code matching against closure codes (`QRALC`, `QRTCA`, `QFAHC`, `QFALC`) combined with free-text regex scanning for phrases like "GPS JAMMING," "GNSS INTERFERENCE," "MILITARY ACTIVITY," "AIRSPACE CLOSED," and "AD CLSD." This dual-filter approach catches both properly coded and free-text-only conflict NOTAMs.

The PostgreSQL/PostGIS architecture maps naturally to NOTAM data. Each NOTAM carries coordinates and a radius that translate directly to PostGIS geometry columns, enabling spatial queries like "all active NOTAMs within 500km of Kyiv." TimescaleDB handles the temporal dimension — NOTAM effective start/end times form natural time-series data for trend analysis. The OGC Testbed-17 Aviation API project validated this exact architecture (PostgreSQL + PostGIS + triggers for new NOTAM notifications) for serving NOTAMs as GeoJSON.

---

## Complementary OSINT sources beyond NOTAMs

NOTAM data alone provides only part of the conflict monitoring picture. **ADS-B Exchange** (`adsbexchange.com`) is the most OSINT-friendly flight tracker, providing unfiltered data including military aircraft that other trackers suppress. **GPSJam.org** derives daily global GPS interference maps from ADS-B data by analyzing Navigation Integrity Category (NIC) and Navigation Accuracy Category (NACp) values — this detects GPS jamming whether or not a NOTAM has been issued. **Safe Airspace** (`safeairspace.net`) by OPSGROUP provides human-curated conflict zone intelligence aggregating NOTAMs with analyst commentary, though it lacks a public API. **OpenSky Network** provides open ADS-B data (free for academic/government research) but does not carry NOTAM data.

---

## Conclusion: a three-source architecture on Hetzner

The optimal strategy combines **three free sources** with **one affordable commercial API**. Start with the **FAA SWIM/SCDS** feed for real-time US NOTAM streaming into your PostgreSQL/PostGIS database — the reference client at `github.com/faa-swim/fns-client` provides the architecture template, and the data covers US military GPS testing NOTAMs and international flight prohibitions (KZZZ designator). Add the **NATS UK XML feed** via hourly polling for European GPS jamming coverage, and build similar scrapers for other national AIS contingency feeds as you discover them. Layer in **Notamify** ($24.90/month + credits) for global coverage of conflict zones outside US/European airspace — its historical archive enables trend analysis, and as a European company it aligns with hosting preferences.

Skip the ICAO API (cost-prohibitive for continuous monitoring), Eurocontrol B2B/EAD (inaccessible to non-aviation entities), and Laminar Data (enterprise pricing). The NASA DIP API is a useful free supplement for structured US data without the SWIM/JMS complexity. Build your NOTAM parser in Rust using `nom` for text and `quick-xml` for AIXM XML, referencing the `svoop/notam` Ruby gem for the most complete ICAO parsing logic. Store everything in PostGIS with spatial indexes on NOTAM geometry, and run conflict detection using Q-code matching plus free-text regex against the WorldMonitor patterns. This architecture runs comfortably on a modest Hetzner VPS while providing near-real-time global conflict airspace awareness.
