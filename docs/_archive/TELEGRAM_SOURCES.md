# OSINT Telegram channels for conflict monitoring

**More than 80 Telegram channels across five conflict zones and dozens of analytical communities form the backbone of open-source military intelligence today.** This guide catalogs the most valuable channels for a conflict monitoring platform, organized by theater, credibility, and automation potential. Telegram has become the de facto real-time intelligence wire for modern conflicts — the Ukraine war alone spawned an ecosystem of milblogger channels with billions of cumulative views, while the Middle East conflict theater relies on structured alert bots pulling directly from government APIs. The channels below were verified as active in 2024–2025 and carry established reputations within the OSINT community.

---

## Ukraine and Russia: the largest OSINT ecosystem on Telegram

The Russia-Ukraine war has produced the most developed Telegram OSINT infrastructure of any modern conflict, with channels spanning frontline mapping, equipment loss tracking, combat footage geolocation, and air raid alerting.

### Ukrainian-side channels

**DeepState UA** (@DeepStateUA for Ukrainian, @DeepStateEN for English) operates the most widely used frontline map of the war at deepstatemap.live. With **~789,000 subscribers** on the Ukrainian channel and a team of ~100 people, it signed a memorandum of cooperation with Ukraine's Ministry of Defense in March 2024. The map has surpassed 1 billion total views. DeepState applies deliberate OPSEC delays and is cited by BBC, ISW, and major Western outlets. It is an original reporting and analytical channel with very high credibility for frontline positions, though it carries a pro-Ukrainian perspective.

**Truha⚡️Ukraine** (@trukhaukraineUA) is the largest Ukrainian Telegram channel at **~2.27 million subscribers** — a breaking-news aggregator covering air raid alerts, missile strikes, and political developments. It is the fastest-breaking Ukrainian news source but its aggregator nature means some unverified content slips through.

**General Staff of the Armed Forces of Ukraine** (@GeneralStaffZSU, ~500,000+ subscribers) provides official daily operational reports and equipment loss claims in Ukrainian. The **Main Directorate of Intelligence (HUR)** (@DIUkraine) publishes intelligence assessments in Ukrainian and English.

For **real-time air threats**, two volunteer-run channels stand out. **Nikolaev Vanek** (~400,000+ subscribers) provides near-real-time tracking of missile launches, drone attacks, and air defense activity. **Monitor** (Монітор) offers 24-hour coverage monitoring "two to ten radio waves" plus 1,000+ social media sources. Both are life-critical information sources for Ukrainian civilians.

The **official air alarm channel** (@air_alert_ua), created in March 2022 by Ajax Systems, publishes machine-parseable start/end alerts per region — this is the core data source feeding multiple APIs and apps used by **11 million+ users** through the AirAlert ecosystem.

**Ukrainian brigade channels** provide unmatched ground-truth combat footage. The **3rd Separate Assault Brigade** (@3ShturM) is described by Kyiv Post as having "combat footage and frontline information second to none." The **47th Mechanized Brigade** (Bradley-equipped) and **36th Marine Brigade** also maintain active channels with original frontline content.

Among Ukrainian journalists, **Yury Butusov** (@butusov_plus) is arguably Ukraine's leading combat correspondent, while **Andriy Tsaplienko** (@tsaplienko) provides fast, wide-reaching aggregation from the front.

### Russian-side channels

**Rybar** (@rybar for Russian, @rybar_in_english for English) is the most influential Russian military analytical channel with **~1.5 million subscribers**. Founded by Mikhail Zvinchuk, a former Russian MoD press service employee, it publishes 5–6 detailed frontline reports daily with high-quality maps and claims 150+ volunteer OSINT researchers. CNN, Bloomberg, and ISW all cite Rybar extensively — a single ISW report may contain 20+ Rybar references during major battles. The U.S. government offered a **$10 million reward** for information about its employees in October 2024. Rybar is openly pro-Russian but attempts analytical objectivity in military assessments.

**Operatsiya Z / Voenkory Russkoy Vesny** (@RVvoenkor) is the most-viewed Russian milblogger channel, accumulating nearly **2.5 billion views** in a three-month period tracked by the Alliance for Securing Democracy. It is a collective channel run by Russian war correspondents — high engagement but heavy propaganda.

**Colonel Cassad** (@boris_rozhin, ~865,000+ subscribers) is one of the longest-running Russian military blogs, run by Boris Rozhin from Crimea. **Dva Majora** (@dva_majors) provides detailed tactical analysis from two anonymous military analysts. **Grey Zone** (@grey_zone, ~310,000+ subscribers) was linked to the Wagner Group and provided unique PMC operational perspectives. **WarGonzo** (Semyon Pegov) delivers frontline war correspondence — Kyiv Post notes he is "biased but often an accurate window into tactics and equipment used by Russian soldiers."

**Yuriy Podolyaka** (@yurasumy) is one of the most popular Russian war commentators, generating **67+ million reactions** in a three-month GMF tracking period. **Military Observer** (@milinfolive) is frequently cited by Western analysts including Rob Lee of FPRI.

### Equipment loss tracking

**Oryx** (oryxspioenkop.com) remains the gold standard for visually confirmed military equipment losses, documenting **20,000+ Russian** and **7,600+ Ukrainian** units lost with photographic evidence for each entry. **WarSpotting** applies even stricter methodology with unit identification and geolocation. Both operate primarily via websites and X/Twitter, with data widely shared across Telegram. **Andrew Perpetua** tracks daily losses that are "usually proven right by Oryx and WarSpotting at a later date" and occasionally purchases satellite imagery for areas of suspected heavy losses. **LostArmour** (lostarmour.info) is a Russian-run tracker useful only for occasionally capturing Ukrainian losses missed by other sources.

### Ukrainian OSINT community channels

Several Ukrainian OSINT channels — **OSINT Пчелы (OSINT Bees)**, **Cat Eyes OSINT**, **OSINT Flow**, and **KRIG War OSINT Analytics** — were temporarily blocked by Telegram in June 2025 before being restored within hours. This incident highlights platform risk for Telegram-dependent monitoring.

---

## Middle East: missile alerts, multi-front warfare, and militant media

### Israel missile alert systems — ideal for automated parsing

**Cumta Red Alerts** (@CumtaAlertsEnglishChannel, ~23,100 subscribers) is the premier English-language missile alert channel, relaying real-time Pikud HaOref (Home Front Command) alerts. Each message follows a **fully structured template** with timestamp, region, city list, regional councils, and alert type (rocket fire, UAV intrusion, etc.). A companion bot (@CumtaAlertsEnglishBot) allows location-based alerts. The developer (@morha13) also provides Android app and Chrome extension integrations. This channel rates **5/5 for automation suitability** — its consistent formatting makes it trivially parseable.

**TzevAdomBot** (@TzevAdomBot) provides location-based Tzeva Adom alerts with **open-source code on GitHub** (RobotTrick/TzevAdomBot), enabling self-hosting and modification. The underlying **Pikud HaOref API** can be accessed directly — open-source implementations include RedAlertPy and RedAlert for Home Assistant.

### Israeli and regional channels

**IDF Official** (@idfofficial, estimated 500,000+ subscribers) publishes official military announcements and operational updates in English. **Abu Ali Express** (@englishabuali, Hebrew version ~420,000+ followers) is Israel's most-viewed Telegram channel per post, covering the full spectrum of Middle East conflicts across Arabic, Persian, Hebrew, and English sources. A significant credibility caveat: Haaretz revealed in 2022 that its operator Gilad Cohen worked as a consultant for the IDF's Influencing Department (psychological operations). It remains extremely rapid at breaking news but carries a clear pro-Israeli editorial slant.

**OsintIL** (@osintil) serves the Hebrew-language Israeli OSINT community. **Suriyakmaps** (@Suriyak_maps, 105,700+ X followers) produces interactive Google Maps for Syria, Iraq, **Yemen**, Libya, the Sahel, Gaza, and Ukraine — its dedicated **Yemen Civil War Map** with frontline tracking and coordinate data makes it valuable for geospatial analysis.

### Houthi and Red Sea monitoring

**Ansar Allah Media Center** (@Ansarallah_MC) is the official Houthi military channel publishing attack claims on shipping, drone/missile strike announcements, and operational footage in Arabic. The Institute for Strategic Dialogue (ISD) mapped **98 entities** in the Houthi digital ecosystem in 2024: 60 official and 38 informal accounts across 22 Telegram channels. Houthi channel reach increased dramatically following Red Sea attacks starting November 2023. **Al-Masirah**, the Houthi media outlet, operates Arabic and English Telegram channels, though the Arabic version contains significantly more detailed military content.

For maritime security, dedicated Telegram channels are less common — most Red Sea OSINT flows through **UKMTO** (UK Maritime Trade Operations at ukmto.org), general conflict aggregators, and Houthi official channels. UKMTO issues structured incident reports with coordinates and vessel details.

### Iranian and Hezbollah channels

**IRGC Cyber** (@Sepahcybery) provides the Iranian Revolutionary Guard Corps perspective in Farsi. The **Hezbollah Military Media Unit** operates channels publishing real-time operation claims, weapon system announcements, and UAV surveillance footage in Arabic. Subscriber bases grew significantly after October 7, 2023. Handle changes are common due to periodic Telegram enforcement.

**Resistance News Network** (@PalestineResist) aggregates Palestinian resistance operations in English and Arabic, with backup channels (@RNN_Backup) and archives (@RNN_Archive). Pro-Iranian axis channels including **@Sabereenp1** (Iraqi militia) and **@WilayatAlFaqih12** cross-reference operations across the Iran-backed network.

A widely circulated list by OSINT analyst Justen Charters identifies 20 key Telegram channels for Middle East conflict monitoring, including channels for Hamas, Palestinian Islamic Jihad, Al-Qassam Brigades, and various resistance factions. **Legal caution is essential** — many of these are channels of designated terrorist organizations, and monitoring should follow appropriate legal frameworks using standard OSINT tradecraft (burner accounts, VPNs).

---

## General military OSINT and multi-conflict aggregators

### English-language aggregators

**NOELREPORTS** (@noel_reports) has published daily situation reports with geolocated footage analysis since February 2022, maintaining **2,150+ Patreon supporters** and high credibility. It covers global conflict zones with primary focus on Ukraine. **War Monitor** (@warmonitors) aggregates breaking news across the Middle East and Ukraine in English. **Clash Report** (@ClashReport) is a well-known multi-language aggregator covering conflicts in English, Turkish, French, and Arabic.

**Intel Slava Z** (@intelslava, ~470,000+ subscribers) is one of the largest English-language conflict aggregators on Telegram — extremely fast at posting breaking news but carries a **strong pro-Russian editorial bias**. Useful for monitoring the Russian information space but should never be treated as neutral reporting.

**SITREP** (@sitreports) is an independent OSINT channel publishing map reports, video analyses, and situation updates in English.

### Military aviation and naval tracking

**Gerjon** (@GerjonFM) tracks NATO and European military aviation movements using ADS-B Exchange data, well-established in the aviation OSINT community. **IntelSky** (@Intelsky, also intelsky.org) provides military flight tracking with a Middle East focus, featuring advanced search by hex code, registration, ICAO type, operator, and squawk codes. **OsintTV** (@OsintTv) covers aviation and geopolitics from an Indian perspective.

ADS-B Exchange (globe.adsbexchange.com) remains the gold-standard tool for unfiltered military flight tracking — unlike FlightRadar24, it does **not filter military or blocked aircraft**. Airplanes.Live is an alternative. For naval tracking, MarineTraffic and VesselFinder provide AIS data, with significant naval movements typically reported across broader OSINT aggregator channels.

### Weapons identification

**Calibre Obscura** (@CalibreObscuraTweets, 115,000+ X followers) is a pseudonymous UK-based researcher contracted by NGOs for weapons-tracking research, specializing in small arms identification in conflict zones. **Ukraine Weapons Tracker** (@UAWeapons) documents Ukrainian and Russian weapons usage with photographic evidence. Both operate primarily on X/Twitter with content forwarded into Telegram.

---

## Verification organizations and established OSINT analysts

### Investigative organizations

**Bellingcat** (@bellingcat_en for English, @bellingcatru for Russian) is the gold standard for OSINT investigative journalism since its 2014 founding by Eliot Higgins. Key investigations include the MH17 downing, Skripal and Navalny poisonings, and Syrian chemical weapons attacks. Bellingcat has won the European Press Prize and Scripps Howard Award, was designated a "foreign agent" in Russia in 2021, and expanded to the US in 2025. It also maintains an **Online Investigation Toolkit** on GitHub cataloging OSINT tools.

**Conflict Intelligence Team (CIT)** (@CITeam for Russian, @CIT_en for English) was founded in 2014 by Ruslan Leviev and specializes in investigating Russian military operations using OSINT and HUMINT. CIT collaborates with BBC, Reuters, Sky News, and Der Spiegel. It was declared an "undesirable organization" in Russia in August 2023, with the team relocating to Georgia.

### Geolocation verification

**GeoConfirmed** (@GeoConfirmed, also geoconfirmed.org) runs a decentralized volunteer network that geolocates and verifies combat footage using satellite imagery, visual landmarks, and metadata analysis. Every geolocation includes visual evidence and detailed reasoning. It is widely cited by the Centre for Information Resilience, Bellingcat, and major media. The **Centre for Information Resilience (CIR)** maps verified civilian harm incidents and maintains searchable databases alongside Bellingcat and GeoConfirmed.

### Individual analysts

**Rob Lee** (@RALee85), Senior Fellow at FPRI, is one of the most-cited Western analysts on the Russian military — he operates primarily on X/Twitter but frequently references Telegram milblogger channels. **Michael Kofman** at the Carnegie Endowment for International Peace is the leading Western expert on Russian military affairs, operating through podcasts and institutional publications. Neither maintains a dedicated Telegram channel, but their analysis draws heavily from and contextualizes Telegram OSINT sources.

### Think tank resources

The **Alliance for Securing Democracy (GMF)** operates a Military Bloggers Dashboard tracking **39 Russian milblogger Telegram channels** with translation, analysis, and visualization — a free interactive tool. The **Institute for the Study of War (ISW)** publishes daily assessments with interactive maps. The **Atlantic Council's DFR Lab** conducts digital forensics research on information operations.

---

## Tools, APIs, and curated lists for building a monitoring platform

### Curated channel directories

The most comprehensive curated list is **Awesome-Telegram-OSINT** on GitHub (ItIsMeCall911/Awesome-Telegram-OSINT), cataloging channel directories, OSINT bots, geolocation tools, and API resources. **The-Osint-Toolbox/Telegram-OSINT** (1,600 stars) provides an in-depth repository covering tools, techniques, and tradecraft with links to analytics platforms. **Ginsberg5150/Discord-and-Telegram-OSINT-references** cross-references key investigation tools.

### Analytics platforms for channel discovery

| Platform | URL | Key Feature | Free Tier |
|----------|-----|-------------|-----------|
| **TGStat** | tgstat.com | 2.67M+ channels indexed, rankings by category/country | Yes (basic) |
| **Telemetrio** | telemetrio.com | Subscriber dynamics, competitive analysis | Yes |
| **Telemetr** | telemetr.io | Engagement analysis per post | Yes |
| **Osavul** | osavul.cloud | 150,000+ channels, AI narrative detection, API access | Enterprise |
| **Flare** | flare.io | AI translation, automated archiving | Enterprise |

### Structured data sources ideal for automated parsing

The **Ukraine Air Raid Alert API** (raid.fly.dev/en, also alerts.com.ua) scrapes @air_alert_ua and provides REST endpoints with SSE real-time streaming. `GET /api/states` returns all 25 regions with alert boolean and timestamp in JSON. A TCP mode on port 1024 supports embedded systems (Arduino, ESP8266). Rate limits are 10 requests/second per IP with free API keys available. The **alerts.in.ua Official API** (devs.alerts.in.ua) is another structured source with five GitHub repositories. The **Pikud HaOref API** for Israeli missile alerts has multiple open-source implementations on GitHub.

### Open-source monitoring tools

**Telepathy** (jordanwildon/Telepathy) is the premier OSINT toolkit, endorsed by Bellingcat, supporting full message history archiving, member scraping, forward mapping for network analysis, and Gephi-compatible output. **Telegram-Tracker** (estebanpdl/telegram-tracker) generates JSON files for channel data with incremental update support. **Telerecon** (sockysec/Telerecon) provides network visualization, ideological indicator parsing, and EXIF geo-mapping. **streaming_overseer** (afolivieri/streaming_overseer) live-monitors channels for keyword matches and forwards to a private channel. All are built on the **Telethon** Python library for the Telegram MTProto API.

---

## Conclusion

Three patterns emerge from this landscape that are critical for building a conflict monitoring platform. First, **the highest-value channels for automation are alert bots** — Cumta Red Alerts, @air_alert_ua, and the underlying Pikud HaOref and Ukraine alarm APIs provide structured, machine-parseable data that can feed directly into monitoring dashboards. Second, **credibility inversely correlates with speed** — the fastest aggregators (Intel Slava, Truha) sacrifice verification for breaking speed, while the most credible sources (Bellingcat, CIT, Oryx) operate on longer timelines. A platform like sitrep.watch should layer both, using fast aggregators for initial detection and verification channels for confirmation. Third, **platform risk is real and growing** — Telegram's June 2025 mass suspension of Ukrainian OSINT channels and Russia's partial blocking of Telegram in February 2026 demonstrate that any monitoring infrastructure must account for channel disappearance and build redundancy across platforms. The GMF Military Bloggers Dashboard, the alerts.in.ua API ecosystem, and tools like Telepathy represent the most mature infrastructure for systematic Telegram OSINT at scale.
