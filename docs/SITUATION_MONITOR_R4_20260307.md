# Situation Monitor R4 — 2026-03-07

Post-deploy monitoring after centroid fixes. DB wiped of situations/incidents/narratives.
App started at http://192.168.1.183:3001. Checking every 60s for 10 rounds.

**Focus areas:**
1. Centroid presence and geographic accuracy
2. UK coords (51.x, -1.x) on non-UK situations (main bug fixed)
3. "Country:" prefix garbage in titles
4. Centroid diversity (unique vs repeated values)

## Monitoring Rounds

### Round 1 — 2026-03-07T23:43:23Z

**Situation count:** 100 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |
|  | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Attack in Unknown Region | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 10
  (31.05,34.85): 48 situations — e.g. 
  (51.77,-1.09): 42 situations — e.g. 
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
  (38.91,-77.04): 1 situations — e.g. Trump Tariff Legal Challenges Reshape Po
  (51.51,-0.13): 1 situations — e.g. Axel Springer Telegraph Acquisition Bid
  (52.52,13.4): 1 situations — e.g. Berlin Film Festival Gaza Boycott Debate
```

**Event ingest (last 1h):**
```
adsb-lol 2348
bgp 2305
adsb-fi 2291
airplaneslive 1933
opensky 1035
rss-news 437
firms 226
notam 27
```

---

### Round 2 — 2026-03-07T23:45:20Z

**Situation count:** 100 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Germany Economic Slowdown|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
Berlin International Film Festival|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| London Aerodrome Closure | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |
| German Customs Enforcement | 51.7722, -1.0928 | developing | 1 | 2 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Gisele Pelicot Drugging Trial | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 10
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
  (38.91,-77.04): 1 situations — e.g. Trump Tariff Legal Challenges Reshape Po
  (51.51,-0.13): 1 situations — e.g. Axel Springer Telegraph Acquisition Bid
  (52.52,13.4): 1 situations — e.g. Berlin Film Festival Gaza Boycott Debate
```

**Event ingest (last 1h):**
```
bgp 2368
adsb-fi 2269
adsb-lol 2135
airplaneslive 1903
opensky 1049
rss-news 439
firms 226
notam 27
```

---

### Round 3 — 2026-03-07T23:46:23Z

**Situation count:** 100 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Ice Train Disruptions Western Europe|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
Berlin International Film Festival|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |
| German Customs Enforcement | 51.7722, -1.0928 | developing | 1 | 2 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 10
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
  (38.91,-77.04): 1 situations — e.g. Trump Tariff Legal Challenges Reshape Po
  (51.51,-0.13): 1 situations — e.g. Axel Springer Telegraph Acquisition Bid
  (52.52,13.4): 1 situations — e.g. Berlin Film Festival Gaza Boycott Debate
```

**Event ingest (last 1h):**
```
adsb-fi 2342
bgp 2314
adsb-lol 2056
airplaneslive 1969
opensky 1049
rss-news 438
firms 226
notam 27
```

---

### Round 4 — 2026-03-07T23:47:25Z

**Situation count:** 100 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Ice Train Disruptions Western Europe|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Berlin International Film Festival|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| German Catholic Church Leadership | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |
| German Customs Enforcement | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 10
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
  (38.91,-77.04): 1 situations — e.g. Trump Tariff Legal Challenges Reshape Po
  (51.51,-0.13): 1 situations — e.g. Axel Springer Telegraph Acquisition Bid
  (52.52,13.4): 1 situations — e.g. Berlin Film Festival Gaza Boycott Debate
```

**Event ingest (last 1h):**
```
bgp 2277
adsb-fi 2255
adsb-lol 2167
airplaneslive 1870
opensky 1049
rss-news 438
firms 226
notam 27
```

---

### Round 5 — 2026-03-07T23:48:27Z

**Situation count:** 100 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Berlin International Film Festival|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |
| German Customs Enforcement | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 10
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
  (38.91,-77.04): 1 situations — e.g. Trump Tariff Legal Challenges Reshape Po
  (51.51,-0.13): 1 situations — e.g. Axel Springer Telegraph Acquisition Bid
  (52.52,13.4): 1 situations — e.g. Berlin Film Festival Gaza Boycott Debate
```

**Event ingest (last 1h):**
```
adsb-fi 2334
bgp 2236
adsb-lol 2019
airplaneslive 1936
opensky 1042
rss-news 295
firms 226
notam 27
```

---

### Round 6 — 2026-03-07T23:49:30Z

**Situation count:** 100 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Berlin International Film Festival|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Vincent Kompany Managerial Appointment | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 10
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
  (38.91,-77.04): 1 situations — e.g. Trump Tariff Legal Challenges Reshape Po
  (51.51,-0.13): 1 situations — e.g. Axel Springer Telegraph Acquisition Bid
  (52.52,13.4): 1 situations — e.g. Berlin Film Festival Gaza Boycott Debate
```

**Event ingest (last 1h):**
```
bgp 2246
adsb-fi 2223
adsb-lol 2078
airplaneslive 1852
opensky 1042
rss-news 295
firms 226
notam 27
```

---

### Round 7 — 2026-03-07T23:50:46Z

**Situation count:** 117 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Berlin International Film Festival|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Vincent Kompany Managerial Appointment | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 16
  (31.05,34.85): 47 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (16.98,95.44): 11 situations — e.g. Earthquake: Earthquake In Afghanistan, E
  (7.18,30.51): 3 situations — e.g. 
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (6.91,21.86): 1 situations — e.g. Gdacs: Forest Fires In Cameroon, Forest 
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
```

**Event ingest (last 1h):**
```
adsb-fi 2285
bgp 2221
adsb-lol 2000
airplaneslive 1914
opensky 1042
rss-news 440
firms 226
notam 27
```

---

### Round 8 — 2026-03-07T23:51:50Z

**Situation count:** 118 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Berlin International Film Festival|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Vincent Kompany Managerial Appointment | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 16
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (16.98,95.44): 11 situations — e.g. Earthquake: Earthquake In Afghanistan, E
  (7.18,30.51): 3 situations — e.g. 
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (6.91,21.86): 1 situations — e.g. Gdacs: Forest Fires In Cameroon, Forest 
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
```

**Event ingest (last 1h):**
```
bgp 2338
adsb-fi 2195
adsb-lol 2132
airplaneslive 1826
opensky 1042
rss-news 440
firms 226
notam 27
```

---

### Round 9 — 2026-03-07T23:52:53Z

**Situation count:** 128 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Berlin International Film Festival|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Vincent Kompany Managerial Appointment | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 18
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (16.98,95.44): 11 situations — e.g. Earthquake: Earthquake In Afghanistan, E
  (7.18,30.51): 11 situations — e.g. 
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (6.91,21.86): 1 situations — e.g. Gdacs: Forest Fires In Cameroon, Forest 
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
```

**Event ingest (last 1h):**
```
bgp 2326
adsb-fi 2259
adsb-lol 2040
airplaneslive 1892
opensky 1042
rss-news 440
firms 226
notam 27
```

---

### Round 10 — 2026-03-07T23:54:13Z

**Situation count:** 128 (API) / 100 (DB)
**With centroid:** 99 | **Without:** 1 | **Unique centroids:** 10

**UK coord bug: DETECTED** on non-UK situations:
```
Berlin International Film Festival|-1.0928|51.7722
Germany Economic Slowdown|-1.0928|51.7722
Ice Train Disruptions Western Europe|-1.0928|51.7722
Scottish Wildfires|-1.0928|51.7722
Vincent Kompany Managerial Appointment|-1.0928|51.7722
Germany Ice Hockey Team|-1.0928|51.7722
ECB Rate Decision|-1.0928|51.7722
Bayern Munich Season Performance|-1.0928|51.7722
Iran Nuclear Diplomacy|-1.0928|51.7722
Paralympic Games Accessibility Dispute|-1.0928|51.7722
Western Europe Energy Crisis|-1.0928|51.7722
EU Sanctions Expansion|-1.0928|51.7722
German Catholic Church Reform|-1.0928|51.7722
German Customs Enforcement|-1.0928|51.7722
Russia Olympic Committee Ban|-1.0928|51.7722
Russian Shadow Fleet North Sea|-1.0928|51.7722
German Catholic Church Leadership|-1.0928|51.7722
Gisele Pelicot Drugging Trial|-1.0928|51.7722
Ketamine Trafficking Western Europe|-1.0928|51.7722
Lyon Flooding|-1.0928|51.7722
Novichok Poisoning Investigation|-1.0928|51.7722
Manchester City Transfer Activity|-1.0928|51.7722
2026 Winter Paralympics|-1.0928|51.7722
Navalny Death Western Response|-1.0928|51.7722
Iran Nuclear Program|-1.0928|51.7722
Axel Springer Management Transition|-1.0928|51.7722
Belarus Paralympic Committee Ban|-1.0928|51.7722
Belgium Political Crisis|-1.0928|51.7722
Serbia EU Membership Talks|-1.0928|51.7722
EU Sanctions Iran Shipping Network|-1.0928|51.7722
Dublin Street Violence|-1.0928|51.7722
Paris Olympics 2024|-1.0928|51.7722
Albania Organized Crime Crackdown|-1.0928|51.7722
Berlin Holocaust Memorial Vandalism|-1.0928|51.7722
ECB Interest Rate Decision|-1.0928|51.7722
Kinahan Cartel Western Europe Operations|-1.0928|51.7722
Axel Springer Telegraph Acquisition Bid|-0.1278|51.5074
Kurdish Opposition Iran Pressure|-1.0928|51.7722
```
**Country: prefix garbage:** None

**Situations with centroids (up to 15):**

| Title | Centroid (lat, lon) | Phase | Sev | Srcs |
|-------|-------------------|-------|-----|------|
| UK Aerodrome Closures | 52.0906, 0.1317 | emerging | 1 | 1 |
| Berlin International Film Festival | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Economic Slowdown | 51.7722, -1.0928 | developing | 1 | 2 |
| Ice Train Disruptions Western Europe | 51.7722, -1.0928 | developing | 1 | 2 |
| Scottish Wildfires | 51.7722, -1.0928 | developing | 1 | 2 |
| Vincent Kompany Managerial Appointment | 51.7722, -1.0928 | developing | 1 | 2 |
| Germany Ice Hockey Team | 51.7722, -1.0928 | developing | 1 | 2 |
| ECB Rate Decision | 51.7722, -1.0928 | developing | 1 | 2 |
| Bayern Munich Season Performance | 51.7722, -1.0928 | developing | 1 | 2 |
| UK Airfield Closures | 51.7722, -1.0928 | developing | 1 | 2 |
| Iran Nuclear Diplomacy | 51.7722, -1.0928 | developing | 1 | 2 |
| EU Sanctions Expansion | 51.7722, -1.0928 | developing | 1 | 2 |
| Western Europe Energy Crisis | 51.7722, -1.0928 | developing | 1 | 2 |
| Paralympic Games Accessibility Dispute | 51.7722, -1.0928 | developing | 1 | 2 |
| Russia Olympic Committee Ban | 51.7722, -1.0928 | developing | 1 | 2 |

**Situations without centroids (up to 10):**

| Title | Centroid | Phase | Sev | Srcs |
|-------|---------|-------|-----|------|
| Gulf of Aden Missile Strikes | NULL | active | 3 | 3 |


**Centroid diversity:**
```
Unique centroid clusters: 18
  (31.05,34.85): 48 situations — e.g. Russia Military Exercises Expansion
  (51.77,-1.09): 42 situations — e.g. Berlin International Film Festival
  (16.98,95.44): 11 situations — e.g. Earthquake: Earthquake In Afghanistan, E
  (7.18,30.51): 11 situations — e.g. 
  (55.38,-3.44): 2 situations — e.g. UK Iran Student Visa Ban
  (6.91,21.86): 1 situations — e.g. Gdacs: Forest Fires In Cameroon, Forest 
  (52.09,0.13): 1 situations — e.g. UK Aerodrome Closures
  (44.79,20.45): 1 situations — e.g. Serbia Elite Investment Network
```

**Event ingest (last 1h):**
```
bgp 2298
adsb-fi 2227
adsb-lol 1959
airplaneslive 1869
opensky 1042
rss-news 440
firms 226
notam 27
```

---

## Summary and Findings

**Monitoring window:** 2026-03-07T23:43:23Z to 2026-03-07T23:54:13Z (10 rounds, ~60s apart)

### Situation Counts

| Metric | Round 1 | Round 10 | Trend |
|--------|---------|----------|-------|
| API situations | 100 | 128 | Growing (coherence splits spawning children) |
| DB situations | 100 | 100 | Stable (children live in-memory or via API join) |
| With centroid | 99 | 99 | Stable |
| Without centroid | 1 | 1 | Stable ("Gulf of Aden Missile Strikes") |
| Unique centroids (DB) | 10 | 10 | Stable |
| Unique centroids (API incl. children) | 10 | 18 | Growing as children get own centroids |

### UK Coord Bug: STILL PRESENT

The main bug we aimed to fix is **still active**. 38 non-UK situations are stuck at Cranfield EGTC NOTAM coordinates (-1.0928, 51.7722). Examples of misplaced situations:

- "ECB Rate Decision" -- should be Frankfurt
- "Gisele Pelicot Drugging Trial" -- should be France
- "Iran Nuclear Diplomacy" -- should be Middle East
- "Germany Economic Slowdown" -- should be Berlin/Germany
- "Berlin International Film Festival" -- should be Berlin
- "Lyon Flooding" -- should be Lyon, France
- "Paris Olympics 2024" -- should be Paris
- "Dublin Street Violence" -- should be Dublin
- "Albania Organized Crime Crackdown" -- should be Albania
- "Serbia EU Membership Talks" -- should be Serbia/Brussels
- "Kurdish Opposition Iran Pressure" -- should be Middle East

**Root cause:** 78 NOTAM events at (-1.0928, 51.7722) from the Cranfield (EGTC) aerodrome closure are being matched into unrelated situations via embedding similarity. The NOTAM events lack topical filtering -- they cluster with RSS news events purely on the basis of embedding proximity. The EWMA centroid calculation is then dominated by the NOTAM coords because most RSS/telegram events lack coordinates. A single Cranfield NOTAM matched to "ECB Rate Decision" sets its centroid to Cranfield because it is the only geolocated event in that situation.

### Second Centroid Cluster Problem

48 situations are pinned to (31.0461, 34.8516) -- Israel coordinates. While some are legitimately Middle East topics, many are not:

- "Russia Military Buildup Eastern Europe" -- should be Eastern Europe
- "Germany Defense Spending Increase" -- should be Germany
- "Hungary EU Energy Dispute" -- should be Hungary/Brussels
- "Spain Defense Ministry Leadership Change" -- should be Spain
- "Viktor Orban Visits Russia" -- should be Russia
- "Friedrich Merz Germany Coalition Talks" -- should be Germany
- "Trump Legal Challenges Escalate" -- should be Washington DC
- "Central African Republic Crisis" -- should be CAR
- "Wolf Population Management Crisis" -- should be Europe

This cluster comes from a mega-situation that absorbed most Middle East + European news events. Its centroid settled on Israel because the Israel/Gaza events had the strongest geo signal from FIRMS/alerts.

### Correctly Geocoded Situations (9 total)

These newer "emerging" situations got correct centroids:

| Situation | Centroid | Correct? |
|-----------|----------|----------|
| Serbia Elite Investment Network | Belgrade (44.79, 20.45) | Yes |
| Trump Tariff Legal Challenges | Washington DC (38.91, -77.04) | Yes |
| Axel Springer Telegraph Acquisition Bid | London (51.51, -0.13) | Yes |
| Berlin Film Festival Gaza Boycott Debate | Berlin (52.52, 13.40) | Yes |
| Ukraine Kharkiv Artillery Strikes | Kharkiv (49.99, 36.23) | Yes |
| UK Iran Student Visa Ban | Scotland (55.38, -3.44) | Yes |
| UK China Espionage Allegations | Scotland (55.38, -3.44) | Yes |
| Iran Oil Market Volatility | Tehran (35.69, 51.39) | Yes |
| UK Aerodrome Closures | Duxford (52.09, 0.13) | Yes |

### Other Issues

1. **Empty titles:** 63 out of 100 DB situations had empty titles at round 1. By round 2, titles populated for most (37 named). The API view eventually showed 103 of 123 children with titles. Title generation is delayed but eventually works.

2. **Country: prefix garbage:** Not detected in any round. This issue appears fixed.

3. **Centroid diversity is poor:** Only 10 unique centroids across 100 DB situations. Two clusters dominate 90% of situations (Cranfield=42, Israel=48). The map view will show two giant clusters with everything piled on top of each other.

4. **Coherence splitting is active:** API count grew from 100 to 128 as the two mega-situations were split into children. Children inherit the parent's (incorrect) centroid.

5. **New child centroids improving:** Some children from GDACS (Cameroon fires at 6.91, 21.86) and earthquake data (Myanmar at 16.98, 95.44) have correct, unique centroids from events with strong geographic signal.

### Root Cause Analysis

The centroid fix works correctly for **new situations created from events with good geo data** (the 9 correctly geocoded situations above). The problem is upstream in event-to-situation matching:

1. **NOTAM events are toxic to clustering.** They have strong location data (exact airport coords) but weak topical content. When matched into a situation via embedding similarity, they dominate the centroid because most RSS/telegram events lack coordinates. One Cranfield NOTAM matched to "ECB Rate Decision" sets its centroid to Cranfield because it is the only geolocated event in that situation.

2. **The mega-situation problem.** One situation absorbed 48+ events spanning Israel, Iran, Europe, Russia, and US topics. Its centroid settled on Israel because the Israel/Gaza events had the strongest geo signal. Coherence splitting breaks it up, but children inherit the bad centroid.

### Recommended Fixes

1. **Exclude NOTAM from centroid calculation** or weight NOTAM locations very low (0.1x) compared to FIRMS/GDACS/seismic events that have inherent geographic meaning.
2. **Filter NOTAM from non-aviation situations** -- add a source-type affinity rule so NOTAM events only cluster with aviation-related situations.
3. **Recompute centroids on coherence split** -- when a child situation is split off, recalculate its centroid from only its assigned events rather than inheriting the parent centroid.
4. **Skip locationless events in centroid EWMA** -- if a situation's only geolocated event is a NOTAM but its topic is "ECB Rate Decision", the centroid should remain NULL rather than defaulting to the NOTAM location.
5. **Tighten embedding similarity threshold for NOTAM** -- NOTAM descriptions ("aerodrome closed to all traffic") are generic enough to match almost anything. Require a higher cosine similarity for NOTAM events to join existing situations.

