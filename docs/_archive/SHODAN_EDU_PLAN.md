# Shodan Edu (Academic) Plan -- Capabilities and Limitations

> **Last updated**: 2026-03-02
> **Context**: Situation Report uses a Shodan edu/academic API key
> **Production error**: `tag:ics` filter returns 400 -- "only available to Corporate API customers and higher"

---

## 1. Shodan Plan Tiers Overview

Shodan has two separate product lines: **Membership** (one-time payment, lifetime)
and **API Subscriptions** (monthly).

### Membership ($49 one-time, lifetime)

| Feature | Value |
|---------|-------|
| Query credits | 100/month |
| Scan credits | 100/month |
| Monitored IPs | 16 |
| Search paging | Yes |
| Streaming API | Alert stream only (monitored IPs) |
| Restricted filters (`tag`, `vuln`) | NO (website-only for `vuln`) |
| Commercial use | Yes |
| Download via website | No |

### Academic Upgrade (free with .edu email)

The academic upgrade is equivalent to a Membership but free for users with
academic email addresses (.edu, .ac.uk, etc.):

| Feature | Value |
|---------|-------|
| Query credits | 100/month |
| Scan credits | 100/month |
| Monitored IPs | 16 |
| `vuln` filter | Website only (NOT via API or CLI) |
| `tag` filter | NO |
| Download via website | No |

### Edu API Subscription (special academic pricing)

Some academic institutions receive a special API subscription plan that
Shodan labels as "edu" in the `/api-info` response. This plan provides
significantly more credits than the free academic upgrade:

| Feature | Value |
|---------|-------|
| Query credits | ~199,999/month |
| Scan credits | ~65,536/month |
| Monitored IPs | ~131,071 |
| Streaming API | Alert stream (monitored IPs) |
| Search paging | Yes |
| Restricted filters (`tag`, `vuln`) | **NO** -- same as Freelancer/Small Business |

This is the plan Situation Report uses. Despite the generous credit allotment
(comparable to Small Business tier), **it does NOT include Corporate-tier
restricted filters**.

### API Subscription Tiers (monthly)

| Feature | Freelancer ($69/mo) | Small Business ($359/mo) | Corporate ($1099/mo) | Enterprise (custom) |
|---------|---------------------|-------------------------|---------------------|---------------------|
| Query credits | ~10,000/mo | ~200,000/mo | Unlimited | Unlimited |
| Scan credits | 5,120/mo | 65,536/mo | 327,680/mo | Custom |
| Monitored IPs | 5,120 | 65,536 | 327,680 | Custom |
| Streaming API | Alert stream | Alert stream | Alert + basic | Full firehose |
| Search paging | Yes | Yes | Yes | Yes |
| `vuln` filter (API) | No | Yes | Yes | Yes |
| `tag` filter (API) | No | No | **Yes** | Yes |
| Batch IP lookups | No | No | Yes | Yes |
| InternetDB commercial | No | No | Yes | Yes |

**Key takeaway**: The `tag` filter is ONLY available on Corporate ($1099/mo)
and Enterprise plans. The `vuln` filter via API is available starting at
Small Business ($359/mo). Our edu plan has neither via the API.

---

## 2. Restricted vs. Available Filters

### Restricted Filters (Corporate+ only)

These filters will return HTTP 400 on edu/Membership/Freelancer/Small Business:

- **`tag`** -- Shodan-assigned tags like `ics`, `vpn`, `database`, `iot`, `malware`
- **`vuln`** -- CVE-based vulnerability search (e.g., `vuln:CVE-2014-0160`)

Note: `vuln` can be used on the Shodan website for academic accounts, but
NOT via the API or CLI. The `tag` filter has no website workaround on academic.

### Available Filters (all plans including edu)

The following filters work on all paid plans and are the ones we use:

**General:**
- `port` -- port number (e.g., `port:502`)
- `country` -- country code (e.g., `country:IR`)
- `city`, `state`, `region` -- geographic
- `org` -- organization name
- `asn` -- AS number
- `net` -- CIDR range (e.g., `net:1.2.3.0/24`)
- `hostname`, `ip` -- host lookup
- `product` -- software product name
- `version` -- software version
- `os` -- operating system
- `isp` -- ISP name
- `geo` -- lat,lon,radius (e.g., `geo:35.6,51.4,50`)
- `has_screenshot` -- boolean, has cached screenshot
- `has_ssl` -- boolean, has SSL certificate
- `has_vuln` -- boolean, has known vulnerabilities (different from `vuln:` filter)
- `has_ipv6` -- boolean
- `hash` -- banner hash
- `cpe` -- CPE identifier
- `device` -- device type string
- `link` -- connection type
- `scan` -- scan ID
- `shodan.module` -- crawler module name (e.g., `shodan.module:s7`)

**Screenshots:**
- `screenshot.label` -- ML-assigned label (e.g., `webcam`, `login`, `desktop`)
- `screenshot.hash` -- screenshot perceptual hash

**HTTP:**
- `http.title` -- HTML page title
- `http.html` -- HTML body content
- `http.status` -- HTTP status code
- `http.server_hash` -- server header hash
- `http.favicon.hash` -- favicon hash
- `http.component` -- web technology
- `http.component_category` -- tech category
- `http.waf` -- WAF detection
- `http.robots_hash`, `http.headers_hash`, `http.html_hash`, `http.dom_hash`

**SSL/TLS:**
- `ssl`, `ssl.cert.subject.cn`, `ssl.cert.issuer.cn`, `ssl.jarm`, `ssl.ja3s`
- `ssl.cert.expired`, `ssl.cert.serial`, `ssl.version`, `ssl.cipher.name`

**Protocol-specific:**
- `snmp.name`, `snmp.contact`, `snmp.location`
- `ssh.hassh`, `ssh.type`
- `ntp.ip`, `ntp.port`
- `telnet.do`, `telnet.option`
- `bitcoin.ip`, `bitcoin.version`

**Cloud:**
- `cloud.provider`, `cloud.region`, `cloud.service`

---

## 3. Rate Limits

All API plans (including edu) are subject to:

- **1 request per second** for the REST API
- No explicit rate limit on the Streaming API (it is a persistent connection)
- 429 responses include a `Retry-After` header

Our code enforces 1-second delays between search queries and 500ms between
consecutive discovery queries.

---

## 4. ICS/SCADA Discovery Strategy (Edu-Compatible)

### The Problem

Previously, many of our discovery queries used `tag:ics country:XX` which is
a convenient Shodan meta-tag that identifies industrial control systems
regardless of protocol. However, this filter requires the Corporate plan
($1099/mo) and fails with HTTP 400 on our edu plan.

### The Solution: Port-Based ICS Discovery

Instead of relying on the `tag:ics` meta-filter, we query for specific
ICS/SCADA protocol ports directly. This is actually MORE precise because
we know exactly which protocol we are targeting.

#### Core ICS/SCADA Ports

| Port | Protocol | Description |
|------|----------|-------------|
| 102 | S7comm | Siemens S7 PLCs (ISO-TSAP) |
| 502 | Modbus | Most common ICS protocol |
| 789 | Red Lion Crimson | Red Lion HMI/RTU |
| 1089-1091 | FF HSE | Foundation Fieldbus |
| 1911 | Niagara Fox | Tridium/Honeywell building automation |
| 2222 | EtherNet/IP | Implicit messaging |
| 2404 | IEC 60870-5-104 | Power grid telecontrol |
| 4840 | OPC UA | Modern ICS interop standard |
| 4911 | Niagara Fox SSL | Fox protocol over TLS |
| 9600 | OMRON FINS | OMRON PLCs |
| 18245 | GE SRTP | GE Fanuc PLCs |
| 20000 | DNP3 | SCADA, water/power utilities |
| 34962-34964 | PROFINET | Siemens industrial Ethernet |
| 44818 | EtherNet/IP | Explicit messaging (CIP) |
| 47808 | BACnet | Building automation |
| 55553 | Honeywell CEE | Honeywell DCS |
| 55555 | Crestron CTP | AV control systems |

#### Current Discovery Queries

Organized by region and priority for conflict monitoring:

**Iran (Priority 1-2):**
- `port:502 country:IR` -- Modbus (most prolific ICS protocol)
- `port:102 country:IR` -- Siemens S7comm
- `port:20000 country:IR` -- DNP3
- `port:47808 country:IR` -- BACnet
- `port:44818 country:IR` -- EtherNet/IP
- `port:2404 country:IR` -- IEC 60870-5-104
- `port:4840 country:IR` -- OPC UA
- `port:1911 country:IR` -- Niagara Fox
- `port:9600 country:IR` -- OMRON FINS
- `port:34962 country:IR` -- PROFINET
- `"Schneider Electric" country:IR` -- Product name search
- `"Siemens" port:102 country:IR` -- Vendor + protocol

**Gulf States (Priority 2):**
- `port:502 country:AE/SA/BH/QA/KW` -- Modbus
- `port:102 country:AE/SA` -- S7comm
- `port:47808 country:AE/SA` -- BACnet

**Israel (Priority 1-2):**
- `port:502 country:IL` -- Modbus
- `port:102 country:IL` -- S7comm
- `port:47808 country:IL` -- BACnet
- `port:44818 country:IL` -- EtherNet/IP
- `port:2404 country:IL` -- IEC 60870-5-104

**Iraq/Syria/Lebanon (Priority 3):**
- `port:502 country:IQ/SY/LB` -- Modbus
- `port:102 country:IQ` -- S7comm

**Maritime (Priority 2-3):**
- `port:502 org:"port"` -- Port-facility Modbus
- `"NMEA" country:IR` -- Maritime navigation
- `"Kongsberg" country:IR` -- Maritime systems vendor

#### ShodanSearch Count Polling Queries

The ShodanSearch source polls `/shodan/host/count` hourly to track ICS
exposure trends. All queries are port-based:

- Iran: port:502, port:102, port:47808, port:44818, port:20000, port:2404
- Israel: port:502, port:102, port:47808
- Ukraine: port:502, port:102
- Saudi Arabia: port:502, port:47808
- UAE: port:502, port:47808
- Iraq: port:502

---

## 5. Advanced Edu-Compatible Queries for Conflict Monitoring

Beyond basic port scanning, these patterns work on the edu plan and
are useful for situational awareness:

### Product/Vendor-Based Discovery

```
"Schneider Electric" country:IR
"Siemens" port:102 country:IR
"ABB" country:IR
"Honeywell" country:IR
"Yokogawa" country:IR
"Emerson" country:IR
"Rockwell" country:IR
"Allen-Bradley" country:IR
```

### Network Infrastructure

```
port:179 country:IR          -- BGP routers
port:53 country:IR           -- DNS servers
port:161 country:IR          -- SNMP
port:1883 country:IR         -- MQTT IoT broker
```

### Shodan Module Queries (edu-compatible)

The `shodan.module` filter is NOT restricted and can identify specific
protocol scanners used by Shodan:

```
shodan.module:s7 country:IR          -- S7comm devices
shodan.module:modbus country:IR      -- Modbus devices
shodan.module:bacnet country:IR      -- BACnet devices
shodan.module:dnp3 country:IR        -- DNP3 devices
shodan.module:ethernetip country:IR  -- EtherNet/IP devices
shodan.module:iec-104 country:IR     -- IEC 104 devices
shodan.module:fox country:IR         -- Niagara Fox devices
```

These are functionally equivalent to `tag:ics` for the specific protocol
and may yield cleaner results than port-only queries.

### Camera/Screenshot Intelligence

```
has_screenshot:true screenshot.label:webcam geo:35.6,51.4,100    -- Tehran area
has_screenshot:true screenshot.label:webcam country:IR
has_screenshot:true country:IR                                    -- Any device with screenshot
screenshot.label:login country:IR                                 -- Login pages
```

### SSL Certificate Intelligence

```
ssl.cert.issuer.cn:"National Informatics Centre" country:IR
ssl.cert.expired:true port:443 country:IR
ssl.cert.subject.cn:"scada" country:IR
```

### Geo-Fenced Queries

```
geo:35.6962,51.4231,50 port:502     -- Tehran 50km radius, Modbus
geo:27.1833,56.2667,30 port:502     -- Bandar Abbas / Strait of Hormuz
geo:32.0853,34.7818,30 port:502     -- Tel Aviv area
```

---

## 6. Credit Budget Considerations

### Edu Plan Budget (per month)

| Resource | Budget | Notes |
|----------|--------|-------|
| Query credits | ~199,999 | 1 credit = 1 search page (100 results) |
| Scan credits | ~65,536 | 1 credit = 1 IP scan |
| Monitored IPs | ~131,071 | Alert stream slots |

### Our Current Usage

- **ShodanDiscovery**: Runs daily, capped at 1,000 credits per run.
  With ~40 queries at 3 pages each = ~120 credits per run.
  Monthly: ~3,600 credits (1.8% of budget).

- **ShodanSearch**: Runs hourly, 16 count queries per run.
  Count endpoint costs 1 credit per query with filters.
  Monthly: ~11,520 credits (5.8% of budget).

- **ShodanCameraFinder**: On-demand only, 1 credit per search.

- **Total estimated monthly usage**: ~15,120 query credits (~7.6% of budget).

### Credit-Free Operations

These operations do NOT consume query credits:

- `/shodan/host/{ip}` -- Host lookup (free, unlimited)
- InternetDB (`internetdb.shodan.io`) -- Free, no auth required
- `/shodan/host/count` without filters -- Free
- Streaming API connection -- Free (uses monitored IP slots, not credits)
- Alert CRUD operations -- Free

---

## 7. What We CANNOT Do on Edu

1. **`tag:ics`** -- Cannot use the tag filter (Corporate+ only)
2. **`vuln:CVE-xxx`** -- Cannot search by CVE via API (Small Business+ for API)
3. **Full firehose** -- Cannot access `stream.shodan.io/shodan/banners` (Enterprise only)
4. **Batch IP lookups** -- Not available (Corporate+ only)
5. **Data downloads via website** -- Academic accounts cannot download from web UI

### Workarounds

| Restricted Feature | Edu Workaround |
|-------------------|----------------|
| `tag:ics` | Port-based queries (port:502, port:102, etc.) |
| `tag:ics` | `shodan.module:modbus`, `shodan.module:s7`, etc. |
| `vuln:CVE-xxx` | `has_vuln:true` (boolean, works on all plans) |
| `vuln:CVE-xxx` | Check `vulns` field in banner response data |
| Full firehose | Alert stream (`stream.shodan.io/shodan/alert`) |
| Batch lookups | Sequential `/shodan/host/{ip}` (free, no credits) |

---

## 8. Streaming API Details

### Alert Stream (available on edu)

Endpoint: `GET https://stream.shodan.io/shodan/alert?key={API_KEY}`

- Returns banners for all monitored IPs in real-time
- NDJSON format (one JSON object per line)
- Persistent HTTP connection (no timeout)
- Heartbeat: empty lines sent periodically
- Contains full banner data including tags, vulns, location

Our `ShodanStream` source connects to this endpoint and processes banners
as they arrive, emitting `InsertableEvent` objects for each banner.

### Country/Port Streams (NOT available on edu)

These require Freelancer+ subscription:
- `stream.shodan.io/shodan/ports/{port}` -- Banners for specific port
- `stream.shodan.io/shodan/countries/{CC}` -- Banners for specific country

### Full Firehose (Enterprise only)

- `stream.shodan.io/shodan/banners` -- ALL banners (1200-1500/sec)

---

## 9. Recommended Future Enhancements

### Use `shodan.module` for Better ICS Detection

Consider adding `shodan.module` queries alongside port queries. The module
filter identifies devices that Shodan's protocol-specific crawlers confirmed
as running the target protocol, reducing false positives from other services
that happen to run on ICS ports.

### InternetDB for Free Enrichment

`https://internetdb.shodan.io/{ip}` is completely free, requires no API key,
and returns ports, CPEs, hostnames, tags, and vulns for any IP. Use this to
enrich discovered IPs without consuming query credits.

### Host Lookup for Deep Inspection

`/shodan/host/{ip}` is free and returns full banner history. After discovery
identifies interesting IPs, use this endpoint for deep inspection without
consuming credits.

### Credit Monitoring

The `/api-info` endpoint returns current credit balances. Our code already
checks this before discovery runs. Consider adding a periodic health check
that logs credit usage trends.
