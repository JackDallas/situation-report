# Deployment Timeline — 2026-03-03

## Deploy 1 — 01:30 UTC (initial)
Fresh deploy with all source fixes, ADSB aggregator, Telegram grammers, BGP dedup, pipeline tuning.
- DB wiped clean, all sources came up
- Telegram: 12/14 channels resolved, 473 messages backfilled, real-time streaming started
- ADSB: all 3 sources (airplaneslive, adsb-fi, adsb-lol) producing data
- **Problem discovered**: 193 situations, 163 were flight-only callsign junk (e.g., "Flight SCRCH54")

## Hot Fix 1 — ~10:15 UTC (flight clustering)
- Flight positions can NO LONGER create new situation clusters (only merge into existing)
- ADSB sources treated as same source for diversity scoring (was inflating cross-source counts)
- `effective_source_diversity()` helper collapses flight sources into 1
- Result: 0 flight-only situations

## Hot Fix 2 — ~10:25 UTC (enrichment + entity filtering)
- Enrichment expanded to Telegram + GeoNews (was NewsArticle only) — Ollama only, no Claude fallback
- Telegram channel names rejected as entities (e.g., "War Monitor" is a source, not an entity)

## Hot Fix 3 — ~10:30 UTC (FIRMS clustering)
- FIRMS same-source threshold lowered to 2 (was 4) — fires merge by region + geo proximity
- Result: fires in same region (Israel/Lebanon/Syria) merge into 1 cluster instead of 10

## Deploy 3 — 10:30 UTC (current)
Situations wiped, events preserved (~145k), pipeline rebuilding from 6h backfill.

## Timeline

| Time (UTC) | Events | Sources Active | Situations | Enriched | Narratives | Budget | Notes |
|---|---|---|---|---|---|---|---|
| 10:34 | 146285 | 11/26 | 1 (emerging:1) | 0/0 | 1 rpt:0 | $0 | tg:473 pos:2087 logs:0
?E/20W |

### Check #1 — 2026-03-03 10:34 UTC
**Events total**: 146285 | **Recent (15m)**: adsb-fi:817,airplaneslive:762,adsb-lol:693,bgp:567,opensky:185,rss-news:143,firms:33,shodan:16
**All sources**: adsb-fi:32923,bgp:31431,airplaneslive:31251,adsb-lol:29975,opensky:8408,shodan:5541,firms:4095,rss-news:1251,geoconfirmed:705,telegram:473
**Situations**: 1 | **Phases**: emerging:1
**Enrichment**: 0 / 0 news | **Narratives**: 1 | **Reports**: 0
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2087
**Failing sources**: none
**Docker logs**: 0
? errors, 20 warnings

| 10:49 | 150272 | 7/26 | 307 (declining:137,emerging:96,developing:59,active:15) | 0/0 | 52 rpt:1 | $0 | tg:473 pos:2130 logs:0
?E/56W |

### Check #2 — 2026-03-03 10:49 UTC
**Events total**: 150272 | **Recent (15m)**: adsb-fi:1071,airplaneslive:1035,adsb-lol:945,bgp:488,opensky:279,rss-news:147,notam:7
**All sources**: adsb-fi:33994,airplaneslive:32286,bgp:31919,adsb-lol:30920,opensky:8687,shodan:5541,firms:4095,rss-news:1416,geoconfirmed:705,telegram:473
**Situations**: 307 | **Phases**: declining:137,emerging:96,developing:59,active:15
**Enrichment**: 0 / 0 news | **Narratives**: 52 | **Reports**: 1
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2130
**Failing sources**: none
**Docker logs**: 0
? errors, 56 warnings

| 11:04 | 155026 | 8/26 | 418 (declining:192,emerging:95,developing:87,active:44) | 0/0 | 18 rpt:0 | $0 | errs: firms(2); tg:473 pos:2175 logs:2E/37W |

### Check #3 — 2026-03-03 11:04 UTC
**Events total**: 155026 | **Recent (15m)**: bgp:1183,adsb-fi:1113,airplaneslive:1078,adsb-lol:1013,opensky:345,ooni:3,cloudflare:2,rss-news:2
**All sources**: adsb-fi:35107,airplaneslive:33364,bgp:33102,adsb-lol:31933,opensky:9032,shodan:5541,firms:4095,rss-news:1430,geoconfirmed:705,telegram:473
**Situations**: 418 | **Phases**: declining:192,emerging:95,developing:87,active:44
**Enrichment**: 0 / 0 news | **Narratives**: 18 | **Reports**: 0
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2175
**Failing sources**: firms(2)
**Docker logs**: 2 errors, 37 warnings

## Hot Fix 4 — ~11:05 UTC (news clustering)
- Removed source_reliability score multiplier entirely — was halving scores for RSS/GDELT (0.50-0.55×), making same-source threshold of 7 mathematically unreachable
- Lowered RSS/GDELT same-source threshold from 7 to 5
- Relaxed news-only merge conditions: added `shared_region && shared_topics >= 3` path and lowered topic_jaccard threshold from 3→2 shared topics
- Result: 5000-event backfill produces 2 situations (was 390). After 5min live: 6 situations total (4 top-level), with 2 child situations properly merged under parent

## Deploy 4 — 11:06 UTC (current)
Situations wiped, events preserved (~155k), pipeline rebuilding with fixed clustering.

| 11:19 | 162494 | 8/26 | 45 (developing:37,emerging:6,active:2) | 0/0 | 15 rpt:0 | $0 | errs: firms(3); tg:473 pos:2219 logs:1E/116W |

### Check #4 — 2026-03-03 11:19 UTC
**Events total**: 162494 | **Recent (15m)**: bgp:4264,adsb-fi:969,airplaneslive:904,adsb-lol:893,opensky:275,rss-news:144,notam:7,cloudflare:2
**All sources**: bgp:37366,adsb-fi:36076,airplaneslive:34268,adsb-lol:32826,opensky:9307,shodan:5541,firms:4095,rss-news:1584,geoconfirmed:705,telegram:473
**Situations**: 45 | **Phases**: developing:37,emerging:6,active:2
**Enrichment**: 0 / 0 news | **Narratives**: 15 | **Reports**: 0
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2219
**Failing sources**: firms(3)
**Docker logs**: 1 errors, 116 warnings

| 11:34 | 166193 | 7/26 | 57 (developing:41,emerging:14,active:2) | 0/0 | 60 rpt:0 | $0 | errs: firms(3); tg:473 pos:2254 logs:0
?E/141W |

### Check #5 — 2026-03-03 11:34 UTC
**Events total**: 166193 | **Recent (15m)**: adsb-fi:919,airplaneslive:877,adsb-lol:874,bgp:702,opensky:298,shodan:16,ooni:3
**All sources**: bgp:38067,adsb-fi:36995,airplaneslive:35145,adsb-lol:33700,opensky:9605,shodan:5557,firms:4095,rss-news:1595,geoconfirmed:705,telegram:473
**Situations**: 57 | **Phases**: developing:41,emerging:14,active:2
**Enrichment**: 0 / 0 news | **Narratives**: 60 | **Reports**: 0
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2254
**Failing sources**: firms(3)
**Docker logs**: 0
? errors, 141 warnings

| 11:49 | 170971 | 8/26 | 79 (developing:42,emerging:33,active:2,declining:2) | 0/0 | 97 rpt:0 | $0 | errs: firms(5); tg:473 pos:2307 logs:2E/139W |

### Check #6 — 2026-03-03 11:49 UTC
**Events total**: 170971 | **Recent (15m)**: bgp:1657,adsb-lol:996,adsb-fi:926,airplaneslive:867,opensky:306,notam:7,rss-news:3,cloudflare:2
**All sources**: bgp:39729,adsb-fi:37921,airplaneslive:36012,adsb-lol:34696,opensky:9911,shodan:5557,firms:4095,rss-news:1607,geoconfirmed:705,telegram:473
**Situations**: 79 | **Phases**: developing:42,emerging:33,active:2,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 97 | **Reports**: 0
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2307
**Failing sources**: firms(5)
**Docker logs**: 2 errors, 139 warnings

## Hot Fix 5 — ~11:51 UTC (FIRMS key + Telegram session)
- Regenerated .env from 1Password (`op inject`) — adds TELEGRAM_API_ID, TELEGRAM_API_HASH, new FIRMS_MAP_KEY, EXA_API_KEY, ANTHROPIC_API_KEY
- FIRMS: Old key was JWT (Earthdata login token), new key is correct short hex MAP_KEY → 230 events on first poll
- Telegram: Session file wasn't persisting across container recreates (docker cp'd into old container, not on bind mount). Copied to `~/situationreport/data/` on host, which bind-mounts to `/app/data/`
- Telegram: 12/14 channels resolved, real-time streaming active. 2 not found: GeoConfirmed, Ansarallah_MC
- All source failures cleared: 0 consecutive failures across all sources
- 8 sources active in last 5 min: bgp, adsb-fi, airplaneslive, adsb-lol, firms, opensky, rss-news, notam

| 12:04 | 175633 | 10/26 | 166 (developing:90,active:39,emerging:35,declining:2) | 0/0 | 114 rpt:0 | $0 | tg:473 pos:2354 logs:0
?E/45W |

### Check #7 — 2026-03-03 12:04 UTC
**Events total**: 175633 | **Recent (15m)**: adsb-fi:1124,airplaneslive:1008,adsb-lol:867,bgp:727,opensky:396,rss-news:288,firms:230,notam:7
**All sources**: bgp:40456,adsb-fi:39045,airplaneslive:37020,adsb-lol:35563,opensky:10307,shodan:5557,firms:4325,rss-news:1905,geoconfirmed:705,telegram:473
**Situations**: 166 | **Phases**: developing:90,active:39,emerging:35,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 114 | **Reports**: 0
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2354
**Failing sources**: none
**Docker logs**: 0
? errors, 45 warnings

| 12:19 | 179264 | 6/26 | 556 (active:427,developing:90,emerging:37,declining:2) | 0/0 | 193 rpt:1 | $0 | tg:473 pos:2393 logs:0
?E/2W |

### Check #8 — 2026-03-03 12:19 UTC
**Events total**: 179264 | **Recent (15m)**: adsb-fi:961,airplaneslive:879,adsb-lol:838,bgp:551,opensky:385,notam:7
**All sources**: bgp:41007,adsb-fi:40006,airplaneslive:37899,adsb-lol:36401,opensky:10692,shodan:5557,firms:4325,rss-news:1915,geoconfirmed:705,telegram:473
**Situations**: 556 | **Phases**: active:427,developing:90,emerging:37,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 193 | **Reports**: 1
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2393
**Failing sources**: none
**Docker logs**: 0
? errors, 2 warnings

| 12:49 | 188025 | 5/26 | 568 (active:430,developing:96,emerging:40,declining:2) | 0/0 | 289 rpt:2 | $0 | tg:473 pos:2487 logs:?E/?W |

### Check #9 — 2026-03-03 12:49 UTC
**Events total**: 188025 | **Recent (15m)**: adsb-fi:1064,adsb-lol:1037,airplaneslive:979,bgp:806,opensky:285
**All sources**: bgp:42831,adsb-fi:42070,airplaneslive:39808,adsb-lol:38290,opensky:11333,shodan:5573,firms:4555,rss-news:2088,geoconfirmed:705,telegram:473
**Situations**: 568 | **Phases**: active:430,developing:96,emerging:40,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 289 | **Reports**: 2
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2487
**Failing sources**: none
**Docker logs**: ? errors, ? warnings

| 13:21 | 199051 | 6/26 | 575 (active:430,developing:97,emerging:46,declining:2) | 0/0 | 417 rpt:4 | $0 | tg:473 pos:2628 logs:0
?E/468W |

### Check #10 — 2026-03-03 13:21 UTC
**Events total**: 199051 | **Recent (15m)**: adsb-fi:1334,airplaneslive:1275,adsb-lol:1117,bgp:662,opensky:335,firms:2
**All sources**: bgp:45052,adsb-fi:44953,airplaneslive:42565,adsb-lol:40595,opensky:12113,shodan:5573,firms:4589,rss-news:2134,geoconfirmed:705,telegram:473
**Situations**: 575 | **Phases**: active:430,developing:97,emerging:46,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 417 | **Reports**: 4
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2628
**Failing sources**: none
**Docker logs**: 0
? errors, 468 warnings


## Hot Fix 6 — ~13:29 UTC (incident noise + log spam)
- Correlation rules `infra_attack` and `coordinated_shutdown` were firing on every restart from routine co-occurrence of BGP/Shodan/OONI data during 6h backfill
- Raised evidence thresholds: infra_attack needs 2+ Shodan, 3+ BGP, 2+ outage (was 1 each); coordinated_shutdown needs 3+ BGP, 2+ outage, 2+ censorship (was 1 each)
- Lowered severity from Critical to High for both rules
- **Frontend**: Removed pipeline incidents from situation list entirely — they only show as AlertBanner toasts (auto-dismiss 30s). Situation list now shows only curated backend clusters.
- **Frontend**: Removed "incidents always sort first" rule from situation sort
- **Log spam**: Phase transition blocked messages downgraded from INFO to DEBUG (was 4290 messages per 5 minutes)
- Result: situation list clean of internet outage noise, logs dramatically quieter
| 13:51 | 209991 | 8/26 | 588 (active:430,developing:104,emerging:52,declining:2) | 0/0 | 514 rpt:6 | $0 | tg:473 pos:2750 logs:0
?E/52W |

### Check #11 — 2026-03-03 13:51 UTC
**Events total**: 209991 | **Recent (15m)**: adsb-fi:1539,airplaneslive:1501,adsb-lol:1317,bgp:678,opensky:355,firms:230,notam:7,rss-news:1
**All sources**: adsb-fi:47903,bgp:46530,airplaneslive:45414,adsb-lol:43043,opensky:12891,shodan:5589,firms:4819,rss-news:2306,geoconfirmed:705,telegram:473
**Situations**: 588 | **Phases**: active:430,developing:104,emerging:52,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 514 | **Reports**: 6
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2750
**Failing sources**: none
**Docker logs**: 0
? errors, 52 warnings

| 14:21 | 222881 | 7/26 | 591 (active:431,developing:103,emerging:55,declining:2) | 0/0 | 689 rpt:8 | $0 | tg:473 pos:2907 logs:0
?E/51W |

### Check #12 — 2026-03-03 14:21 UTC
**Events total**: 222881 | **Recent (15m)**: airplaneslive:2111,adsb-fi:1884,adsb-lol:1715,bgp:641,opensky:350,firms:50,rss-news:3
**All sources**: adsb-fi:51509,airplaneslive:49181,bgp:47858,adsb-lol:46466,opensky:13575,shodan:5589,firms:4869,rss-news:2338,geoconfirmed:705,telegram:473
**Situations**: 591 | **Phases**: active:431,developing:103,emerging:55,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 689 | **Reports**: 8
**Budget**: $0 | **Telegram**: 473 | **Positions**: 2907
**Failing sources**: none
**Docker logs**: 0
? errors, 51 warnings

| 14:51 | 238451 | 6/26 | 596 (active:431,developing:108,emerging:55,declining:2) | 0/0 | 706 rpt:10 | $0 | tg:473 pos:3111 logs:?E/?W |

### Check #13 — 2026-03-03 14:51 UTC
**Events total**: 238451 | **Recent (15m)**: adsb-fi:2098,airplaneslive:2007,adsb-lol:1956,bgp:1543,opensky:357,firms:2
**All sources**: adsb-fi:55952,airplaneslive:53154,adsb-lol:50394,bgp:50309,opensky:14293,shodan:5605,firms:4871,rss-news:2352,geoconfirmed:705,telegram:473
**Situations**: 596 | **Phases**: active:431,developing:108,emerging:55,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 706 | **Reports**: 10
**Budget**: $0 | **Telegram**: 473 | **Positions**: 3111
**Failing sources**: none
**Docker logs**: ? errors, ? warnings

| 15:23 | 254232 | 6/26 | 597 (active:431,developing:108,emerging:56,declining:2) | 0/0 | 709 rpt:12 | $0 | tg:473 pos:3328 logs:0
?E/48W |

### Check #14 — 2026-03-03 15:23 UTC
**Events total**: 254232 | **Recent (15m)**: airplaneslive:2270,adsb-fi:2269,adsb-lol:2204,bgp:686,opensky:283,firms:28
**All sources**: adsb-fi:60553,airplaneslive:57726,adsb-lol:54820,bgp:51783,opensky:14947,shodan:5605,firms:4899,rss-news:2374,geoconfirmed:705,telegram:473
**Situations**: 597 | **Phases**: active:431,developing:108,emerging:56,declining:2
**Enrichment**: 0 / 0 news | **Narratives**: 709 | **Reports**: 12
**Budget**: $0 | **Telegram**: 473 | **Positions**: 3328
**Failing sources**: none
**Docker logs**: 0
? errors, 48 warnings

| 15:53 | 280903 | 7/26 | 715 (active:448,emerging:157,developing:108,resolved:1,declining:1) | 0/0 | 731 rpt:13 | $0 | tg:473 pos:3477 logs:0
?E/47W |

### Check #15 — 2026-03-03 15:53 UTC
**Events total**: 280903 | **Recent (15m)**: firms:9551,airplaneslive:2617,adsb-fi:2545,adsb-lol:2402,bgp:646,opensky:283,rss-news:3
**All sources**: adsb-fi:65647,airplaneslive:62888,adsb-lol:59607,bgp:53148,opensky:15611,firms:14450,shodan:5621,rss-news:2404,geoconfirmed:706,telegram:473
**Situations**: 715 | **Phases**: active:448,emerging:157,developing:108,resolved:1,declining:1
**Enrichment**: 0 / 0 news | **Narratives**: 731 | **Reports**: 13
**Budget**: $0 | **Telegram**: 473 | **Positions**: 3477
**Failing sources**: none
**Docker logs**: 0
? errors, 47 warnings

| 16:24 | 298758 | 9/26 | 166 (emerging:72,developing:50,active:44) | 0/0 | 38 rpt:14 | $0 | tg:967 pos:3621 logs:0
?E/47W |

### Check #16 — 2026-03-03 16:24 UTC
**Events total**: 298758 | **Recent (15m)**: airplaneslive:2369,adsb-fi:2305,adsb-lol:2161,bgp:532,opensky:342,firms:230,rss-news:144,notam:7
**All sources**: adsb-fi:70750,airplaneslive:68141,adsb-lol:64362,bgp:54296,opensky:16312,firms:14680,shodan:5621,rss-news:2566,telegram:967,geoconfirmed:706
**Situations**: 166 | **Phases**: emerging:72,developing:50,active:44
**Enrichment**: 0 / 0 news | **Narratives**: 38 | **Reports**: 14
**Budget**: $0 | **Telegram**: 967 | **Positions**: 3621
**Failing sources**: none
**Docker logs**: 0
? errors, 47 warnings

| 17:24 | 25342 | 7/26 | 5 (emerging:5) | 0/0 | 0 rpt:0 | $0 | tg:525 pos:850 logs:0
?E/49W |

### Check #17 — 2026-03-03 17:24 UTC
**Events total**: 25342 | **Recent (15m)**: airplaneslive:2280,adsb-fi:2275,adsb-lol:2137,bgp:623,opensky:244,firms:50,ooni:4
**All sources**: airplaneslive:5424,shodan:5403,adsb-fi:5349,adsb-lol:4992,bgp:1371,rss-news:705,geoconfirmed:700,opensky:576,telegram:525,firms:280
**Situations**: 5 | **Phases**: emerging:5
**Enrichment**: 0 / 0 news | **Narratives**: 0 | **Reports**: 0
**Budget**: $0 | **Telegram**: 525 | **Positions**: 850
**Failing sources**: none
**Docker logs**: 0
? errors, 49 warnings

| 18:24 | 58270 | 8/26 | 278 (declining:124,emerging:99,developing:53,active:2) | 0/0 | 12 rpt:3 | $0 | tg:525 pos:1342 logs:0
?E/49W |

### Check #18 — 2026-03-03 18:24 UTC
**Events total**: 58270 | **Recent (15m)**: airplaneslive:2537,adsb-fi:2492,adsb-lol:2360,bgp:586,opensky:143,firms:50,ooni:3,rss-news:3
**All sources**: airplaneslive:15455,adsb-fi:15113,adsb-lol:14276,shodan:5419,bgp:3870,opensky:1297,rss-news:1014,geoconfirmed:700,firms:560,telegram:525
**Situations**: 278 | **Phases**: declining:124,emerging:99,developing:53,active:2
**Enrichment**: 0 / 0 news | **Narratives**: 12 | **Reports**: 3
**Budget**: $0 | **Telegram**: 525 | **Positions**: 1342
**Failing sources**: none
**Docker logs**: 0
? errors, 49 warnings

| 19:24 | 312152 | 9/26 | 211 (emerging:98,developing:62,active:51) | 0/0 | 181 rpt:16 | $0 | errs: gdelt-geo(1),gdelt(1); tg:967 pos:3900 logs:2E/94W |

### Check #19 — 2026-03-03 19:24 UTC
**Events total**: 312152 | **Recent (15m)**: adsb-fi:734,airplaneslive:714,adsb-lol:666,firms:230,bgp:81,opensky:79,shodan:16,notam:6
**All sources**: adsb-fi:74621,airplaneslive:72173,adsb-lol:68021,bgp:55328,opensky:16804,firms:14910,shodan:5653,rss-news:2603,telegram:967,geoconfirmed:706
**Situations**: 211 | **Phases**: emerging:98,developing:62,active:51
**Enrichment**: 0 / 0 news | **Narratives**: 181 | **Reports**: 16
**Budget**: $0 | **Telegram**: 967 | **Positions**: 3900
**Failing sources**: gdelt-geo(1),gdelt(1)
**Docker logs**: 2 errors, 94 warnings

