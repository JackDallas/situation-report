# Additional OSINT Sources - Part 2
## GPS/GNSS Interference, NOTAMs, Nighttime Lights, Economic Indicators, Telegram Monitoring & Nuclear Detection

*Research Date: 1 March 2026*
*Context: Iran-Israel-US conflict monitoring (Feb 28 2026 strikes)*

---

## 1. GPS/GNSS Jamming & Spoofing Detection

### Why This Matters
GPS interference is both a **leading indicator** and a **force multiplier** in modern conflicts. Iran has been spoofing GPS across the Persian Gulf for years, causing civilian aircraft to report incorrect positions. During active military operations, GPS jamming intensifies dramatically as both sides attempt to degrade the other's precision-guided munitions and drone navigation. Detecting interference zones tells you where operations are about to happen or are actively happening.

GPS interference also degrades the accuracy of every other geolocation source we're using — ADS-B positions, AIS ship tracking, and even FIRMS hotspot coordinates can be affected when GPS is compromised.

### Primary Sources

#### GPSJam.org
- **URL**: https://gpsjam.org
- **Data Source**: ADS-B Exchange aircraft reports
- **Method**: Maps hexagonal zones where significant percentages of aircraft report low navigation accuracy (NIC/NAC values)
- **Update Frequency**: Daily, updated around midnight UTC (manual process by creator John Wiseman @lemonodor)
- **Cost**: Free, no API (scraping required)
- **Historical Data**: From 14 February 2022 onwards
- **Coverage**: Global, but only where aircraft with ADS-B are flying and ADS-B Exchange has receivers
- **Colour Coding**: Green (normal), Yellow (some interference), Red (significant interference)
- **Limitations**:
  - Measures jamming well but NOT spoofing (spoofed aircraft don't know their NIC is degraded — that's the whole point of spoofing)
  - 24-hour aggregation means short interference windows still colour hexes red
  - Aircraft altitude means ground-level interference may differ
  - No API — data must be scraped from the map
  - Occasional data gaps due to power outages, network issues at data collection site (e.g., 37-hour outage during Jan 2025 LA wildfires)
- **Conflict Zones Typically Showing Interference**: Eastern Mediterranean, Iraq, Lebanon, Cyprus, Turkey, Armenia, Poland, Romania, Baltics, Libya

#### GPSwise / SkAI Data Services Spoofing Tracker
- **URL**: https://gpswise.aero (also https://spoofing.skai-data-services.com)
- **Data Source**: OpenSky Network ADS-B data
- **Developer**: SkAI Data Services + Zurich University of Applied Sciences (ZHAW) Centre for Aviation
- **Method**: Detects spoofing specifically by checking whether multiple aircraft report being at the same location simultaneously and examining altitude inconsistencies
- **Update Frequency**: Near real-time
- **Cost**: Free web map; custom API endpoints available (contact SkAI for pricing)
- **Display Features**:
  - Clusters showing numbers of spoofed flights at each location
  - Blue markers showing aircraft positions just before being spoofed
  - Lines connecting real positions to spoofed locations
  - Hexagonal heatmap for jamming (colour-coded by NIC severity)
  - Adjustable time window slider
- **Data Persistence**: Last 48 hours (limited due to server capacity)
- **Limitations**: Dependent on OpenSky Network ADS-B coverage (less coverage than ADS-B Exchange)
- **Known Spoofing Hotspots**: Sevastopol (Black Sea), Beirut (Eastern Med/Israel), Cairo (Egypt) — aircraft GPS "thinks" it's at these locations

#### Flightradar24 GPS Jamming Map
- **URL**: https://www.flightradar24.com/data/gps-jamming
- **Update Frequency**: Every 6 hours
- **Cost**: Free to view
- **Advantage**: Larger ADS-B receiver network than OpenSky

### Intelligence Value & Cross-Reference Strategy

**Pre-strike Indicator**: GPS jamming often intensifies 1-12 hours before military operations as electronic warfare assets activate. A sudden expansion of interference zones over the Persian Gulf, Iranian airspace, or Iraqi airspace can indicate imminent strikes.

**Cross-reference with**:
- **NOTAMs** (Section 2): Airspace closures + GPS interference = military operation almost certain
- **HFGCS/EAM volume** (from ADDITIONAL-SOURCES.md): EAM surge + GPS interference spike = strategic-level operation
- **ADS-B flight tracking**: Military aircraft may disappear from ADS-B in areas of heavy jamming
- **AIS maritime data**: Ships in Hormuz reporting GPS anomalies, position jumps, or going dark

**Current Situation (Feb 28 2026)**:
- UKMTO has specifically warned of "elevated electronic interference, including disruption to AIS and other navigational systems" in the Arabian Gulf, Gulf of Oman, North Arabian Sea, and Strait of Hormuz
- Vessel position jumps averaging 6,300km during jamming events (Q1 2025 data; likely far higher now)
- GPS interference is actively being used to degrade Iranian C2 and to complicate coalition navigation

### Integration Notes
- GPSJam.org can be scraped daily and the hex data stored for time-series analysis
- GPSwise/SkAI may offer API access for integration — worth contacting
- Monitor for sudden expansion of red zones over Iran, Persian Gulf, or Iraqi airspace
- Compare interference patterns between pre-strike, during-strike, and post-strike periods

---

## 2. NOTAMs (Notices to Air Missions)

### Why This Matters
NOTAMs are the single most predictive publicly-available signal for military operations. Before every airstrike campaign, the attacking force must close airspace to protect civilian aircraft. These closures are published as NOTAMs and are **machine-readable, public, and often appear hours before the first bombs fall**.

The Feb 28 2026 US-Israel strikes on Iran would have been preceded by NOTAMs closing airspace over Iraq (transit corridors), the Persian Gulf, and potentially parts of Iranian airspace. Iran itself also issues NOTAMs for its military exercises and retaliatory operations.

### Key NOTAM Types for Conflict Monitoring

| NOTAM Type | Significance |
|---|---|
| **Temporary Restricted Areas (TRAs)** | Airspace closed for military operations. Large/unusual TRAs = imminent strikes |
| **Danger Areas** | Zones where military activity (live fire, missile launch) is occurring |
| **Route Closures** | ATS routes closed, forcing civilian diversions = military corridor established |
| **Airport Closures** | Military airfield status changes, civilian airports closed for force protection |
| **Navigation Aid Outages** | GPS/VOR/DME degradation notices (complements GPSJam data) |

### Key ICAO FIR Codes to Monitor

| FIR | Code | Coverage |
|---|---|---|
| Tehran FIR | OIIX | Iranian airspace |
| Baghdad FIR | ORBB | Iraqi airspace (transit corridor for US strikes) |
| Bahrain FIR | OBBB | Persian Gulf, Bahrain, eastern Saudi Arabia |
| Emirates FIR | OMAE | UAE, Gulf of Oman |
| Muscat FIR | OOMM | Oman, Arabian Sea |
| Jeddah FIR | OEJD | Western Saudi Arabia, Red Sea |
| Ankara FIR | LTBB | Turkey (Incirlik base area) |
| Nicosia FIR | LCCC | Cyprus, eastern Mediterranean |
| Damascus FIR | OSTT | Syrian airspace |
| Amman FIR | OJAC | Jordan (possible transit corridor) |
| Tel Aviv FIR | LLLL | Israeli airspace |

### NOTAM Data Sources

#### FAA Federal NOTAM System (FNS)
- **URL**: https://notams.aim.faa.gov/notamSearch/
- **Coverage**: Global (via Defense Internet NOTAM Service - DINS)
- **Cost**: Free
- **Format**: Web search interface, text-based NOTAM format
- **Limitation**: No public API; web scraping required

#### ICAO API Data Service
- **URL**: https://applications.icao.int/dataservices/
- **Cost**: Free registration, 100 free API calls, then paid booster packs
- **Format**: JSON/CSV
- **Coverage**: Global (authoritative ICAO source via DINS)
- **API Key**: Required, obtained via registration
- **Endpoint Pattern**: Query by FIR code (e.g., OIIX for Tehran)

#### Eurocontrol/EAD (European AIS Database)
- **Coverage**: European and Middle Eastern NOTAMs
- **Access**: Via Eurocontrol EAD, requires registration
- **Note**: Authoritative source for European NOTAMs; some Middle East data available

#### Autorouter NOTAM API
- **URL**: https://api.autorouter.aero/v1.0/notam
- **Format**: JSON
- **Query**: By ICAO identifier (airport or FIR code)
- **Cost**: Free (appears to be open)
- **Example**: `GET /v1.0/notam?itemas=["OIIX"]&offset=0&limit=100`
- **Parameters**: startvalidity, endvalidity (Unix epoch), offset, limit (max 100)
- **Source**: Eurocontrol EAD via INO system

#### Notamify
- **URL**: https://notamify.com/notam-api
- **Cost**: Credit-based pricing (trial rates from $15)
- **Features**: AI-powered categorisation (42 categories), smart scheduling, atomic elements breakdown
- **V2 Endpoint**: Enhanced interpretation data, automatic classification
- **Watcher API**: Set up email alerts for specific NOTAMs (runway closures, restricted airspace, etc.)
- **Best For**: Automated monitoring with intelligent filtering

#### Laminar Data (Cirium)
- **URL**: https://developer.laminardata.aero/documentation/notamdata/v2
- **Format**: GeoJSON with feature geometries
- **Coverage**: Global
- **Features**: Machine-readable geometries for each NOTAM, Q-line parsing, vertical limits
- **Cost**: Registration required, likely paid for production use

#### Aviation Edge NOTAM API
- **URL**: https://aviation-edge.com/notam-api/
- **Cost**: Paid API (trial from $15)
- **Features**: Real-time, searchable by location

#### AvDelphi (Removed)
Remove, to expensive

### Intelligence Application

**Pre-strike Detection Pattern**:
1. Large TRA/Danger Area NOTAMs appear over Iraq, Persian Gulf, or Jordan (transit corridors)
2. Combined with GPS interference reports from GPSJam
3. Combined with military aircraft activity on ADS-B (tankers, ISR assets)
4. Civilian airline diversions visible on flight tracking
5. → High confidence strikes imminent within 2-12 hours

**Iranian Retaliation Warning Pattern**:
1. NOTAMs from OIIX (Tehran FIR) closing airspace over missile launch areas
2. Combined with HFGCS EAM surge
3. Combined with AIS ships reporting diversions from Hormuz
4. → High confidence Iranian missile launch imminent

**Monitoring Strategy**:
- Poll for new NOTAMs on key FIRs every 15-30 minutes
- Filter for: Q-code starting with QR (restricted areas), QW (warnings), QA (airfield), QN (navigation)
- Alert on: Any new TRA/Danger Area in OIIX, ORBB, OBBB, OMAE
- Cross-reference with flight tracking to confirm civilian diversions

### Integration Priority: **TIER 1 - IMMEDIATE**
NOTAMs are the most actionable predictive signal available. Recommend starting with the Autorouter API (free, JSON, FIR-queryable) and supplementing with FAA FNS scraping.

---

## 3. VIIRS Nighttime Lights / NASA Black Marble

### Why This Matters
When power infrastructure is destroyed, cities go dark. Iran at 4% internet connectivity (per Cloudflare/IODA data) almost certainly means massive power grid destruction. NASA's VIIRS Day/Night Band can see this from space — it's the visual counterpart to IODA's connectivity data, confirming infrastructure destruction at a scale that ground truth cannot.

This is especially valuable for:
- Confirming which Iranian cities/regions have lost power
- Tracking the geographic spread of infrastructure damage over time
- Detecting oil fires (massive thermal signatures at night)
- Monitoring recovery (lights coming back on = infrastructure restoration)

### Data Products

#### VNP46A2 — Daily Gap-Filled BRDF-Corrected Nighttime Lights (PRIMARY)
- **Source**: VIIRS instrument on Suomi NPP, NOAA-20, NOAA-21 satellites
- **Resolution**: 500m (15 arc-seconds)
- **Temporal**: Daily
- **Latency**: Near real-time (NRT) within 3 hours of acquisition
- **Processing**: Cloud-free, corrected for atmospheric, terrain, lunar BRDF, thermal, and straylight effects
- **Format**: HDF-EOS5
- **Access**:
  - NASA LAADS DAAC: https://ladsweb.modaps.eosdis.nasa.gov/
  - NASA Worldview: https://worldview.earthdata.nasa.gov (browser-based, NRT imagery from July 2025 onwards)
  - Google Earth Engine (daily data available)
  - Copernicus Data Space
- **Authentication**: Free Earthdata Login required (https://urs.earthdata.nasa.gov)

#### VNP46A1 — Daily At-Sensor Top of Atmosphere Nighttime Radiance
- Same sensors/resolution as VNP46A2
- Raw TOA radiance (not gap-filled or BRDF-corrected)
- Useful for seeing unprocessed data when NRT speed matters more than quality

#### Black Marble Nighttime Blue/Yellow Composite
- **False-colour product** designed specifically for power outage detection
- Available on NASA Worldview as NRT layer
- Blue/Yellow colour scheme makes outage areas visually obvious
- Used by NASA for Houston (Hurricane Beryl July 2024), Libya (Derna flooding), etc.

### Access Methods

#### NASA Worldview (Easiest — No Coding)
- **URL**: https://worldview.earthdata.nasa.gov
- Navigate to date of interest, enable "Black Marble" layers
- Compare dates: Feb 27 (pre-strike) vs Feb 28/Mar 1 (post-strike) over Iran
- Export imagery at desired resolution

#### Python — blackmarblepy (World Bank)
```python
# pip install blackmarblepy
from blackmarble.raster import bm_raster
import geopandas as gpd

# Define Iran bounding box as GeoDataFrame
iran_bbox = gpd.GeoDataFrame(geometry=[...], crs="EPSG:4326")

# Download daily NTL raster
ntl = bm_raster(
    roi=iran_bbox,
    product_id="VNP46A2",
    date_range=("2026-02-27", "2026-03-01"),
    bearer=NASA_BEARER_TOKEN  # From Earthdata Login
)
```

#### R — blackmarbler
```r
library(blackmarbler)
ntl_r <- bm_raster(
  roi_sf = iran_sf,
  product_id = "VNP46A2",
  date = "2026-02-28",
  bearer = Sys.getenv("NASA_BEARER")
)
```

#### Google Earth Engine
- Daily VNP46A2 available as ImageCollection
- Can compute difference images (pre-strike minus post-strike) to highlight outage areas
- Free for research/non-commercial use

### Intelligence Application

**Power Outage Mapping**:
- Download VNP46A2 for Feb 27 (baseline pre-strike) and Feb 28 onwards
- Compute pixel-level difference: negative values = lights went out
- Map outage extent over Iranian cities
- Cross-reference with known power plant/grid locations

**Strike Damage Assessment**:
- Oil fires and gas flares produce intense NTL signatures
- Destroyed refineries/storage facilities show thermal anomalies initially, then go dark
- Military base destruction visible as localised light loss
- Urban areas with sustained outages indicate grid-level damage vs localised hits

**Recovery Tracking**:
- Time series of daily NTL over weeks/months shows restoration progress
- Compare with IODA internet connectivity for convergent infrastructure recovery data

**Cross-reference with**:
- **FIRMS** (thermal hotspots): Fire at night = explosion/strike. Fire then darkness = destroyed facility.
- **IODA/Cloudflare** (internet connectivity): Lights out + internet out = confirmed infrastructure destruction
- **USGS seismic** (from ADDITIONAL-SOURCES.md): Seismic event + FIRMS thermal + NTL outage = triple-confirmed strike on infrastructure

### Integration Priority: **TIER 1 - IMMEDIATE**
NASA Worldview requires zero coding and can show Iran power outages within hours. Full pipeline via blackmarblepy enables automated tracking.

---

## 4. Economic & Energy Indicators

### Why This Matters
Financial markets process information faster than any OSINT analyst. Oil futures, shipping insurance rates, and other economic indicators move in real-time and often ahead of confirmed reports. They reflect the aggregate assessment of thousands of market participants with access to commercial satellite data, shipping intelligence, and industry contacts.

The Hormuz closure alone is an economic event of historic proportions — roughly 27% of global crude oil and 22% of LNG trade passes through the Strait.

### Oil Price Data

#### US EIA Open Data API (PRIMARY — FREE)
- **URL**: https://api.eia.gov/v2/
- **Registration**: Free at https://www.eia.gov/opendata/register.php
- **Key Data Series**:
  - `petroleum/pri/spt/data` — Spot prices (Brent, WTI)
  - `petroleum/pri/fut/data` — Futures prices
  - `petroleum/stoc/wstk/data` — Weekly stocks
  - `petroleum/move/imp/data` — Imports by country
  - `steo` — Short Term Energy Outlook (monthly forecasts)
- **Update Frequency**: Weekly (spot/stocks), monthly (outlook)
- **Format**: JSON (default), XML
- **Rate Limits**: Reasonable for most applications
- **Example**:
  ```
  GET https://api.eia.gov/v2/petroleum/pri/spt/data?api_key=YOUR_KEY&data[]=value&facets[series][]=RBRTE&frequency=daily&sort[0][column]=period&sort[0][direction]=desc&length=30
  ```
  Returns last 30 days of daily Brent spot prices.

#### OilPriceAPI
- **URL**: https://api.oilpriceapi.com/v1/prices/latest
- **Cost**: Free tier available; paid plans for historical/streaming
- **Update**: Every 5 minutes
- **Endpoints**:
  - `GET /v1/prices/latest?by_code=BRENT_CRUDE_USD` — Latest Brent price
  - `GET /v1/prices/latest?by_code=WTI_USD` — Latest WTI price
- **Auth**: API key in header (`Authorization: Token YOUR_KEY`)
- **Source**: ICE, CME exchange data via Business Insider
- **WebSocket**: Available on Professional+ plans for real-time streaming

#### Commodities-API
- **URL**: https://commodities-api.com
- **Cost**: Free tier (limited calls)
- **Coverage**: Brent, WTI, natural gas, gold, and many more commodities
- **Format**: JSON with exchange rate conversion in 170+ currencies

#### Trading Economics
- **URL**: https://tradingeconomics.com/commodity/crude-oil
- **Data**: Brent ~$72.87/bbl (Feb 27 close), WTI ~$67.02/bbl
- **Context**: Prices up 2.87% on Feb 27; analysts forecasting $75+ Brent if escalation continues; $120-150 if Hormuz physically disrupted (Kpler estimate)

### Shipping & Insurance Indicators

#### War Risk Insurance Premiums
These are the fastest-moving indicator of maritime risk. Current status:
- **Pre-conflict baseline**: ~0.125% of hull & machinery (H&M) value
- **Post-June 2025 strikes**: Jumped to 0.2-0.4% for Strait of Hormuz transit
- **Israel/US-affiliated vessels**: Quotes up to 0.7% of H&M value
- **VLCC additional cost**: $200,000-$360,000 per voyage
- **Post-Feb 28**: Expected to increase "manyfold" per BIMCO

**Sources for tracking**:
- Lloyd's of London war risk rates (via Lloyd's List, paid)
- S&P Global Commodities at Sea (paid, but reports cited in news)
- BIMCO shipping advisories (https://www.bimco.org)
- UKMTO advisories (free): https://www.ukmto.org
- Joint Maritime Information Centre (JMIC) — daily updates on maritime risk

#### Freight Rates / Baltic Exchange
- **Baltic Dirty Tanker Index (BDTI)**: Tanker freight rates
- **Baltic Clean Tanker Index (BCTI)**: Refined product tanker rates
- **Source**: Baltic Exchange (paid, but reported widely)
- Spike in BDTI = market pricing in Hormuz disruption/rerouting

#### AIS-Derived Shipping Flow Data
- **Kpler** (paid): Real-time crude oil flow tracking, vessel-by-vessel
- **S&P Global Commodities at Sea** (paid): Strait transit volumes
- **Alternative**: Use free aisstream.io (from ADDITIONAL-SOURCES.md) to count vessels transiting Hormuz bounding box and detect flow changes

### Broader Economic Signals

#### Polymarket (Prediction Markets)
- **URL**: https://polymarket.com
- Already documented in ADDITIONAL-SOURCES.md
- Watch: "Will Hormuz be fully reopened by [date]?", oil price target markets
- Prices move faster than news

#### FRED (Federal Reserve Economic Data)
- **URL**: https://fred.stlouisfed.org
- **API**: Free, requires registration
- **Key Series**:
  - DCOILBRENTEU — Brent crude daily
  - DCOILWTICO — WTI daily
  - DFEDTARU — Federal funds rate (if Fed responds to oil shock)
  - Various CDS and financial stress indices

#### CDS Spreads (Credit Default Swaps)
- **What**: Insurance against sovereign debt default
- **Signal**: Spike in CDS for Gulf states = market pricing in regional economic crisis
- **Sources**: Bloomberg Terminal (paid), some data on WorldGovernmentBonds.com
- **Countries to watch**: Iran, Saudi Arabia, UAE, Bahrain, Qatar, Iraq

### Intelligence Application

**Real-time Alert Signals**:
- Brent moves >5% in a session → major supply disruption being priced
- War risk premiums double → insurers have intelligence on specific threat
- Prediction market for Hormuz closure drops below 50% → market believes reopening likely
- VLCC traffic through Hormuz (via AIS) drops >50% from baseline → effective closure regardless of formal status

**Current Situation**:
- Brent at ~$72.87 (+2.87% on Feb 27), expected to test $75+ on Monday
- Analysts: $120-150 if physical disruption (Kpler), $80+ if material supply interruption (Barclays), $100+ in prolonged conflict
- Insurance rates expected to increase "manyfold" — ships with US/Israel business connections may be uninsurable
- OPEC+ meeting Sunday to discuss output response
- Iran claiming Hormuz closure via VHF broadcasts (not legally binding per UKMTO)

### Integration Priority: **TIER 2 — HIGH VALUE**
EIA API (free) for daily oil prices; aisstream.io for Hormuz traffic flow; prediction markets for crowd intelligence. Financial signals often lead OSINT by hours.

---

## 5. Telegram Programmatic Monitoring

### Why This Matters
We've identified key Telegram channels in SOURCES.md (Aurora Intel, OSINTdefender, Fighterman_FANSEN, ASB Military News, Iranian OSINT channels). Currently these require manual monitoring. Programmatic monitoring enables:
- Real-time ingestion of messages from multiple channels simultaneously
- Automated keyword alerting (strike, missile, Natanz, Hormuz, etc.)
- Media capture (photos/videos of strikes for verification)
- Cross-channel correlation (same event reported by multiple sources = higher confidence)
- Time-series analysis of posting frequency (posting spike = event occurring)

### Primary Tool: Telethon

#### Overview
- **Library**: Telethon (Python, asyncio-based)
- **Repository**: https://github.com/LonamiWebs/Telethon
- **Install**: `pip install telethon`
- **License**: MIT
- **Capabilities**: Full Telegram API access — read messages, download media, join channels, iterate history

#### Setup Requirements
1. **Telegram API Credentials**: Register at https://my.telegram.org/apps
   - Obtain `api_id` and `api_hash`
   - Requires a Telegram account (phone number)
   - **Never share credentials or session files**
2. **Session File**: Created automatically on first run, stores authentication
3. **Phone number verification**: Required on first connection

#### Basic Channel Monitor
```python
from telethon import TelegramClient, events
import asyncio

api_id = 'YOUR_API_ID'
api_hash = 'YOUR_API_HASH'

# Channels to monitor
CHANNELS = [
    '@AuroraIntel',
    '@OSINTdefender',
    '@faboratory',       # Fighterman_FANSEN
    '@ASBMilitary',
    '@inaboratory',      # Iran-focused OSINT
    # Add channels from SOURCES.md
]

client = TelegramClient('osint_monitor', api_id, api_hash)

@client.on(events.NewMessage(chats=CHANNELS))
async def handler(event):
    channel = event.chat.title or event.chat.username
    text = event.message.text or ''
    timestamp = event.message.date

    # Keyword alerting
    ALERT_KEYWORDS = [
        'BREAKING', 'strike', 'missile', 'launch', 'explosion',
        'Natanz', 'Fordow', 'Isfahan', 'Hormuz', 'Bandar Abbas',
        'nuclear', 'IRGC', 'scramble', 'intercept', 'NOTAM',
        'GPS jamming', 'AIS', 'dark ship'
    ]

    if any(kw.lower() in text.lower() for kw in ALERT_KEYWORDS):
        print(f"ALERT [{channel}] {timestamp}: {text[:200]}")

    # Download media if attached
    if event.message.media:
        await event.message.download_media(f'media/{timestamp}_{channel}/')

    # Log all messages
    with open('telegram_feed.jsonl', 'a') as f:
        import json
        f.write(json.dumps({
            'channel': channel,
            'timestamp': str(timestamp),
            'text': text,
            'has_media': bool(event.message.media),
            'message_id': event.message.id
        }) + '\n')

async def main():
    await client.start()
    print(f"Monitoring {len(CHANNELS)} channels...")
    await client.run_until_disconnected()

asyncio.run(main())
```

### Ready-Made Tools

#### telegram-scraper (unnohwn)
- **Repository**: https://github.com/unnohwn/telegram-scraper
- **License**: MIT
- **Features**:
  - `[S]` Scrape historical messages from channels
  - `[C]` Continuous real-time monitoring
  - `[M]` Media downloading (photos, documents)
  - `[E]` Export data
  - Maintains state between runs (resume after interruption)
  - Flood control compliance
  - SQLite database storage
  - Progress tracking
- **Setup**: `pip install telethon`, provide api_id/api_hash and phone number
- **Use Case**: Set up continuous scraping on the channels listed in SOURCES.md

#### Alternative Libraries
- **Pyrogram**: Similar to Telethon, some additional features
- **python-telegram-bot**: Bot API (more limited than user API but doesn't require phone number)

### Channels to Monitor (from SOURCES.md + additions)

#### English-Language OSINT
| Channel | Focus |
|---|---|
| @AuroraIntel | Breaking OSINT, global military |
| @OSINTdefender | Conflict monitoring |
| @ASBMilitary | Military news aggregation |
| @faboratory | Flight tracking, military aviation |
| @IntelSlava | Russian-aligned military news |
| @sentdefender | Sentinel defense OSINT |
| @TheIntelLab | Intelligence analysis |

#### Iran/Middle East Specific
| Channel | Focus |
|---|---|
| @IranIntl_Breaking | Iran International breaking news |
| @IranWire | Iranian civil society |
| @ABORSHOMALI | Iran military OSINT (Farsi) |
| @TabnakNews | Iranian news (Farsi, state-adjacent) |
| @TasijilMedia | Houthi/Yemen military media |
| @Hezbollah_Media | Hezbollah operations |

#### Verification Integration
- Use the **media-search-engine** (from ADDITIONAL-SOURCES.md) to check if Telegram strike videos have already been geolocated by GeoConfirmed/Bellingcat/CIR
- Cross-reference Telegram timestamps with FIRMS hotspots and USGS seismic events

### Rate Limits & Ethics
- Telegram API has flood limits — Telethon handles these automatically
- Public channels only — do not scrape private groups without authorisation
- Store messages in local database, not for redistribution
- Comply with Telegram Terms of Service

### Integration Priority: **TIER 2 — HIGH VALUE**
Setting up a continuous Telegram monitor with keyword alerting is the fastest way to get real-time conflict updates. Combined with automated cross-referencing against FIRMS/USGS/ADS-B data, this creates a near-real-time intelligence picture.

---

## 6. Nuclear/Radiological Monitoring

### Why This Matters
Iran's nuclear facilities at Natanz, Fordow, Isfahan (UCF), Arak, and Bushehr are either confirmed or probable targets in the current strikes. If enrichment facilities containing uranium hexafluoride (UF6) or enriched uranium are hit, there is a risk of radiological release — not a nuclear explosion, but dispersal of radioactive material.

Key isotopes of concern:
- **Uranium (U-235, U-238)**: Released if enrichment cascades or UF6 storage are hit
- **Iodine-131**: Released from reactor damage (Bushehr, Arak)
- **Cesium-137**: Released from reactor damage or spent fuel
- **Xenon isotopes (Xe-131m, Xe-133, Xe-133m, Xe-135)**: Noble gases that seep through rock/debris, detected thousands of km away. These are the "smoking gun" for nuclear events.

### Monitoring Networks

#### CTBTO International Monitoring System (IMS)
- **Operator**: Comprehensive Nuclear-Test-Ban Treaty Organization
- **Network**: 80 radionuclide stations worldwide (particulate + 40 noble gas)
- **Capability**: Can detect nuclear explosions AND radiological releases from damaged facilities
- **Detection**: Radioactive noble gases (xenon isotopes) that cannot be contained — they seep through any debris/rubble
- **Latency**: Stations send gamma ray spectra daily; analysis at International Data Centre (IDC) in Vienna
- **Public Access**: **LIMITED** — data restricted to CTBTO Member States' National Data Centres
- **Workaround**: Major detections are announced publicly. Monitor CTBTO press releases and social media
- **Supplementary Technologies**: 50 primary + 120 auxiliary seismic stations; 11 hydroacoustic; 60 infrasound
- **URL**: https://www.ctbto.org/our-work/international-monitoring-system
- **Note**: CTBTO detected the 2020 Beirut explosion using infrasound and seismic data

#### EPA RadNet (United States)
- **URL**: https://www.epa.gov/radnet
- **Dashboard**: Real-time gamma radiation data from 140 monitors in 50 US states
- **Data Access**: https://www.epa.gov/enviro/radnet-overview — searchable database
- **Update**: 24/7 near-real-time gamma measurements + laboratory analysis of air filters
- **Sensitivity**: Can detect trace amounts of radioactive material
- **Historical**: Tracked Chernobyl (1986) and Fukushima (2011) fallout reaching US
- **Relevance**: Would detect any significant radiological release from Iran that reaches US via atmospheric transport (days-weeks after event)
- **Incident Response**: During radiological events, EPA increases sampling frequency and generates accelerated reports
- **Air Filter Inquiry Log**: Public record of when EPA has investigated potential airborne radionuclides
- **Cost**: Free, public data

#### EURDEP (European Radiological Data Exchange Platform)
- **URL**: https://remap.jrc.ec.europa.eu/Advanced.aspx (public advanced map)
- **Operator**: European Commission Joint Research Centre (JRC)
- **Network**: 5,000+ monitoring stations in 38 European countries
- **Data**: Ambient dose rate (ADR) in nSv/h, updated within 2 hours of measurement (most countries)
- **Display**: Colour-coded map showing average and maximum dose rates over last 24 hours and 35 days
- **Coverage**: Europe (would detect any plume reaching European airspace from Iran)
- **Note**: "NON-VALIDATED DATA" — elevated readings can be caused by rain, equipment issues, or calibration. Must cross-reference.
- **GCC-RDEP**: EURDEP variant customised for Gulf Cooperation Council states — may have relevant data for Gulf region
- **Public vs Expert**: Public map has limited features; Expert map (restricted) has full time series and web services
- **Copyright**: All data subject to copyright of original data provider

#### Safecast (Crowdsourced Global)
- **URL**: https://safecast.org / https://map.safecast.org
- **API**: https://api.safecast.org
- **Data**: 150+ million radiation measurements, CC0 public domain
- **Network**: Crowdsourced — volunteers with bGeigie Nano/Zen devices worldwide
- **Coverage**: Strongest in Japan (Fukushima legacy), growing globally
- **Relevance**: Limited coverage near Iran, but any Safecast devices in Gulf states, Turkey, or Central Asia could detect elevated readings
- **Advantage**: Fully open data, no restrictions, API available
- **Use Case**: Baseline comparison — establish normal readings at nearby stations, alert on deviation

#### IAEA Emergency Systems
- **IRMIS**: International Radiation Monitoring Information System — aggregates data from national networks including EURDEP
- **EMERCON**: Emergency notification system for nuclear accidents
- **USIE**: Unified System of Information Exchange for incidents
- **Access**: Restricted to IAEA Member States
- **Public**: IAEA press releases and situation updates during major events

### Detection Timeline (If Natanz/Fordow Hit)

| Timeframe | Detection Method | What's Detected |
|---|---|---|
| **Minutes** | CTBTO infrasound + seismic (if large explosion) | Blast confirmation |
| **Hours** | FIRMS thermal hotspot + USGS seismic | Strike location confirmed |
| **Hours-Days** | Local radiation monitors (if any public in region) | Immediate contamination |
| **Days** | CTBTO radionuclide stations (Kuwait, UAE, Pakistan) | Noble gas detection confirms nuclear material release |
| **Days-Week** | EURDEP stations (Turkey, Cyprus, Greece) | Plume reaching Europe |
| **1-2 Weeks** | EPA RadNet (US) | Trace detection in US atmosphere |

### Atmospheric Transport Modelling
- **HYSPLIT** (NOAA): https://www.ready.noaa.gov/HYSPLIT.php — Free atmospheric dispersion model. Can simulate where a plume from Natanz/Fordow would travel based on current meteorological conditions.
- **ENSEMBLE** (EU): European multi-model atmospheric dispersion system used with EURDEP during nuclear emergencies
- **CTBTO ATM**: Atmospheric Transport Modelling at IDC — backtrack from detection station to source location

### Monitoring Strategy

**Immediate Actions**:
1. Check EPA RadNet Dashboard daily for any Air Filter Inquiry activations
2. Monitor EURDEP public map for elevated readings in Turkey, Cyprus, Gulf states
3. Set up HYSPLIT forward trajectory from Natanz (32.9N, 51.7E) and Fordow (34.88N, 51.59E) using current weather
4. Monitor CTBTO and IAEA social media/press releases for any statements on radiological detection
5. Check Safecast API for any readings in Middle East/Central Asia region

**Alert Indicators**:
- EPA activates enhanced sampling → suspected atmospheric contamination
- EURDEP shows elevated gamma dose rates in Turkey/Cyprus trending upward → plume approaching Europe
- CTBTO announces detection of fission products → confirmed nuclear material release
- IAEA issues EMERCON notification → international-level radiological event

### Integration Priority: **TIER 2 — HIGH VALUE**
EURDEP public map and EPA RadNet Dashboard are free and immediately useful. HYSPLIT modelling can predict plume trajectory. Full CTBTO data requires state-level access but public announcements will be made for significant detections.

---

## Integration Summary — All New Sources

### Tier 1 — Immediate Integration (Free, High Impact)

| Source | Type | Signal | Latency |
|---|---|---|---|
| **NOTAMs** (Autorouter/FAA) | Airspace closures | Predictive — hours before strikes | Minutes |
| **VIIRS/Black Marble** (NASA Worldview) | Nighttime lights | Power outage mapping, damage assessment | Hours |
| **GPSJam.org** | GPS interference | Pre-strike EW activity, operational zones | Daily |

### Tier 2 — High Value (Free-Freemium, Important)

| Source | Type | Signal | Latency |
|---|---|---|---|
| **EIA Oil Price API** | Brent/WTI prices | Economic impact, market perception of risk | Daily |
| **Telegram Monitor** (Telethon) | OSINT channel feeds | Real-time conflict reports, strike videos | Seconds |
| **EURDEP** | Radiation monitoring | Radiological release detection (Europe) | Hours |
| **EPA RadNet** | Radiation monitoring | Radiological contamination (US) | Hours-Days |
| **GPSwise/SkAI** | GPS spoofing detection | Spoofing zones, active EW operations | Near real-time |

### Tier 3 — Supplementary

| Source | Type | Signal | Latency |
|---|---|---|---|
| **Shipping war risk premiums** | Insurance data | Maritime risk level | Days (reporting lag) |
| **HYSPLIT** (NOAA) | Atmospheric modelling | Plume trajectory prediction | Minutes (computation) |
| **Safecast** | Crowdsourced radiation | Baseline radiation comparison | Variable |
| **CTBTO** (public announcements) | Nuclear detection | Confirmed nuclear material release | Days-Weeks |
| **Prediction markets** (Polymarket) | Crowd intelligence | Conflict trajectory | Real-time |

---

## Cross-Reference Matrix — Complete Source Ecosystem

The full source ecosystem across SOURCES.md, FLIGHT-TRACKING.md, ADDITIONAL-SOURCES.md, and this document now enables multi-source confirmation at every stage:

### Pre-Strike Detection
NOTAMs (airspace closure) + GPSJam (EW activity) + ADS-B (military aircraft) + HFGCS (EAM surge) + Telegram (OSINT chatter) + Prediction markets (price spike)

### Strike Confirmation
FIRMS (thermal hotspot) + USGS (seismic) + GeoConfirmed (geolocated video) + Telegram (strike reports) + ACARS (aircraft operational context)

### Damage Assessment
VIIRS/Black Marble (power outages) + IODA/Cloudflare (internet connectivity) + Sentinel-1 SAR (structural damage) + Sentinel-2 (optical imagery) + EURDEP/RadNet (radiological release)

### Maritime Impact
AISstream (vessel traffic) + GPSJam (maritime GPS interference) + War risk premiums (insurance response) + Oil prices (market impact) + UKMTO advisories (official guidance)

### Recovery Tracking
VIIRS nighttime lights (power restoration) + IODA (internet recovery) + BGP/ASN monitoring (network rebuilding) + AIS (shipping resumption) + Oil prices (risk premium decay)
