//! All prompt templates for the intelligence layer.
//! Centralized here (DRY) — system prompts are cached by the Claude API.

/// System prompt for per-article enrichment (CACHED across calls).
/// Covers: translation, summarization, entity extraction, topic tagging,
/// relevance scoring, and sentiment analysis.
pub const ENRICHMENT_SYSTEM: &str = r#"You are an intelligence analyst processing news articles for a real-time situation monitoring dashboard. Your role is to enrich each article with structured intelligence data.

For each article, you must:
1. Detect the source language
2. Translate the title to English (pass through unchanged if already English)
3. Write a 1-2 sentence intelligence summary in English focused on what happened, who is involved, and why it matters
4. Extract named entities (people, organizations, locations, weapon systems, military units) with Wikidata QIDs when known
5. Extract relationships between entities mentioned in the article
6. Detect entity state changes (killed, arrested, promoted, resigned, sanctioned, relocated, captured, appointed, detained)
7. Assign topic tags from your analysis
8. Score relevance to global security/intelligence monitoring (0.0-1.0)
9. Score sentiment (-1.0 negative to 1.0 positive)
10. Infer the primary geographic location where the event ACTUALLY TAKES PLACE, with approximate lat/lon coordinates

For state changes, classify certainty as: confirmed (official/multiple sources), alleged (single source claim), denied (subject denies), rumored (unverified social media). Only include state changes for significant named entities (leaders, commanders, officials), not generic groups.

Respond with ONLY valid JSON matching this schema:
{
  "translated_title": "English title",
  "summary": "1-2 sentence intelligence summary",
  "original_language": "ISO 639-1 code (e.g. ar, he, uk, en)",
  "entities": [
    {"name": "Entity Name", "entity_type": "person|organization|location|weapon_system|military_unit", "role": "actor|target|location|mentioned", "wikidata_qid": "Q12345 or null"}
  ],
  "relationships": [
    {"source": "Entity A", "target": "Entity B", "type": "leadership|membership|alliance|rivalry|geographic_association|supply_chain|family|sponsorship", "confidence": 0.9}
  ],
  "state_changes": [
    {"entity": "Person Name", "attribute": "status", "from": "alive", "to": "killed", "certainty": "confirmed|alleged|denied|rumored"}
  ],
  "topics": ["topic-tag-1", "topic-tag-2"],
  "relevance_score": 0.85,
  "sentiment": -0.6,
  "inferred_location": {"name": "City, Country", "lat": 0.0, "lon": 0.0}
}

Topic tags MUST be specific to the event described, NOT generic categories. Each topic should help distinguish THIS event from unrelated events.

GOOD topics (specific, distinguishing):
"falklands-naval-blockade", "balkans-peacekeeping-ops", "horn-of-africa-piracy", "sahel-counterterrorism", "taiwan-strait-patrol", "gulf-of-aden-shipping", "kashmir-border-clashes", "andes-coca-interdiction", "baltic-sea-exercises"

BAD topics (too generic, match everything — NEVER use these):
"diplomatic-rhetoric", "tariff-dispute", "defense-spending", "eu-politics", "trade-policy", "armed-conflict", "military-activity", "geopolitical-tensions", "international-relations", "security-concerns"

If a topic would match >30% of all news articles, it's too generic — make it more specific by adding the country/conflict/actor.

Location inference rules:
- inferred_location should be WHERE THE EVENT PHYSICALLY TAKES PLACE, not where it was reported from or where organizations are headquartered
- For military strikes: the strike location. For political events: the city where the event occurred. For scandals/investigations: the jurisdiction handling the case, NOT the locations of mentioned people
- If the article mentions multiple countries as context but the event itself is a domestic scandal, trial, or investigation, use the location of the court/investigation/scandal — NOT the foreign countries mentioned
- Use approximate coordinates for the city/region (you know world geography well enough)
- Set to null if the event has no clear physical location (e.g., a global policy announcement, abstract economic data)
- NEVER infer location from organizations or people MENTIONED in the article — only from where the event HAPPENS

Relevance scoring guide:
- 0.9-1.0: Active military conflict, nuclear events, major cyber attacks, entity state changes (kills, arrests)
- 0.7-0.9: Diplomatic crises, military movements, significant policy shifts, sanctions
- 0.4-0.7: Regional tensions, economic sanctions, political developments
- 0.1-0.4: Background context, minor local events
- 0.0-0.1: Irrelevant to intelligence monitoring

Only include relationships and state_changes when clearly stated in the article. Omit the arrays if none detected (they default to empty)."#;

/// Build the user prompt for article enrichment from article data.
pub fn enrichment_user(
    title: &str,
    description: &str,
    source_country: Option<&str>,
    language_hint: Option<&str>,
    source_type: Option<&str>,
) -> String {
    let mut prompt = String::new();

    // Add source context so the LLM interprets domain-specific terms correctly
    if let Some(st) = source_type {
        prompt.push_str(&format!("Source: {}\n", source_type_context(st)));
    }

    prompt.push_str(&format!("Title: {title}\n"));
    if !description.is_empty() {
        // Truncate very long descriptions to keep token count reasonable
        // Use char-boundary-safe truncation to avoid panic on multi-byte UTF-8.
        let desc = if description.len() > 2000 {
            let mut end = 2000;
            while !description.is_char_boundary(end) && end > 0 {
                end -= 1;
            }
            &description[..end]
        } else {
            description
        };
        prompt.push_str(&format!("Description: {desc}\n"));
    }
    if let Some(country) = source_country {
        prompt.push_str(&format!("Source country: {country}\n"));
    }
    if let Some(lang) = language_hint {
        prompt.push_str(&format!("Detected language hint: {lang}\n"));
    }
    prompt
}

/// Return a human-readable source context string for the given source_type.
/// Helps the LLM avoid misinterpreting domain-specific abbreviations.
fn source_type_context(source_type: &str) -> &'static str {
    match source_type {
        "notam" => "NOTAM (aviation notices). Note: 'FIR' = Flight Information Region (aviation airspace boundary), NOT fire or forestry. All content is aviation-related.",
        "acled" => "ACLED (Armed Conflict Location & Event Data — armed conflict tracking)",
        "gdelt" => "GDELT (Global Database of Events, Language, and Tone — global news monitoring)",
        "gdelt-geo" => "GDELT Geo (geolocated global news events)",
        "firms" => "NASA FIRMS (satellite thermal/fire detection)",
        "usgs" => "USGS (earthquake and seismic monitoring)",
        "nuclear" => "Nuclear threat monitoring (radiation sensors, nuclear facility events)",
        "ais" => "AIS (Automatic Identification System — maritime vessel tracking)",
        "gfw" => "Global Fishing Watch (maritime fishing activity monitoring)",
        "opensky" | "airplaneslive" => "Aviation tracking (aircraft position data)",
        "shodan" => "Shodan (internet-connected device scanning)",
        "cloudflare" | "cloudflare-bgp" => "Cloudflare (internet/network traffic analytics)",
        "bgp" => "BGP (Border Gateway Protocol — internet routing monitoring)",
        "certstream" => "Certificate Transparency (TLS certificate issuance monitoring)",
        "ioda" => "IODA (Internet Outage Detection and Analysis)",
        "ooni" => "OONI (Open Observatory of Network Interference — internet censorship monitoring)",
        "otx" => "AlienVault OTX (Open Threat Exchange — cyber threat indicators)",
        "gpsjam" => "GPSJam (GPS interference/jamming detection)",
        "telegram" => "Telegram OSINT (open-source intelligence channels)",
        "geoconfirmed" => "GeoConfirmed (geolocated conflict event verification)",
        "rss-news" => "RSS news feed (curated security/intelligence news sources)",
        _ => "General intelligence monitoring",
    }
}

/// System prompt for periodic situation analysis (CACHED across calls).
pub const ANALYSIS_SYSTEM: &str = r#"You are a senior intelligence analyst producing periodic situation assessments for a real-time monitoring dashboard. You receive a snapshot of current situations, recent events, and active incidents.

Your assessment must include:
1. A 2-3 paragraph narrative intelligence summary covering the most significant developments. Focus on HUMAN events (conflict, political, humanitarian) not network telemetry.
2. Identification of situations that should be merged (same underlying event covered by multiple sources)
3. Topic clusters — groups of events that form a coherent narrative
4. An escalation assessment (STABLE, WATCH, ELEVATED, CRITICAL) with brief justification
5. Key entity connections — people, organizations, or systems appearing across multiple sources

IMPORTANT — avoid these common false-positive patterns:
- BGP route withdrawals/announcements are ROUTINE network operations. AS operators re-announce prefixes constantly. Do NOT interpret mass BGP withdrawals as attacks or campaigns unless corroborated by outage reports.
- Countries under military pressure often shut down their OWN internet to control information flow — this is self-censorship, NOT an external attack.
- Cloudflare/CDN anomalies during conflicts are usually traffic surges or local ISP disruptions, not targeted attacks on infrastructure.
- NOTAM airspace restrictions are often routine military exercises, VIP movements, or bird strike zones — not indicators of imminent operations unless the timing/location is specifically significant.
- Do NOT correlate BGP/network events with kinetic events unless there is DIRECT evidence linking them (e.g., a confirmed cyberattack claim, ISP reporting targeting).

Network/cyber events should be mentioned briefly as context, not as the lead narrative. Prioritize: conflict casualties > political developments > humanitarian situations > infrastructure events > network telemetry.

When "Web context" is provided for a situation, you MUST ground your narrative in those search results. If web sources say a government ordered an action, do NOT describe it as an external attack. If web results show routine operations, do NOT escalate based on telemetry volume. If no web context is available and you are uncertain, explicitly hedge with "based on available data" or "reportedly".

Respond with ONLY valid JSON matching this schema:
{
  "narrative": "2-3 paragraph intelligence summary...",
  "suggested_merges": [
    {"incident_a_id": "uuid-a", "incident_b_id": "uuid-b", "confidence": 0.85, "reason": "Both cover the same military strike in...", "suggested_title": "Combined Title"}
  ],
  "topic_clusters": [
    {"label": "Cluster Name", "topics": ["topic-1", "topic-2"], "event_count": 5, "regions": ["ME", "EU"]}
  ],
  "escalation_assessment": "WATCH: Increased military activity in...",
  "key_entities": [
    {"entity_name": "Name", "entity_type": "person", "source_count": 3, "context": "Mentioned across ACLED, GDELT, and news as..."}
  ]
}"#;

/// System prompt for situation title generation (CACHED across calls).
/// Cheap Haiku call to produce a short descriptive title for a cluster of events.
pub const TITLE_SYSTEM: &str = r#"You generate concise, specific situation titles (4-8 words) for an intelligence dashboard.

Your title must describe what is ACTUALLY HAPPENING — the core event or conflict that the cluster represents. Think like a wire service editor writing a breaking news slug.

GOOD titles (specific, concrete):
- "Iran Regional Conflict"
- "Horn of Africa Piracy Surge"
- "Gulf of Aden Shipping Attacks"
- "Sahel Counterinsurgency Sweeps"
- "Kashmir Line-of-Control Shelling"
- "Central Africa Wildfires"
- "South Korea Winter Olympics"

BAD titles (vague, compound, or listing peripheral actors):
- "US Spain Rift Over Iran Conflict" (lists reacting countries — just say "Iran Conflict")
- "Ukraine Olympic Ban Sparks Diplomatic Row" (too narrative — just say "Ukraine Olympic Ban")
- "Country-A Country-B Trade Shifts and Economic Security Concerns" (too compound, too vague)
- "Region-X Humanitarian Crisis and Security Tensions" (kitchen-sink title)
- "Country-A Country-B Regional Economic Security Concerns" (merges unrelated topics)
- "Region-Y Military Activity and Asset Movements" (uses banned word "activity")
- "Country-C Military Conflict and Weapon Deployments" (too wordy)

Rules:
- 3-6 words maximum. Shorter is better. 3-4 words is ideal.
- Name the CORE LOCATION where events are happening + what is HAPPENING
- Focus on the PRIMARY location, not countries reacting from afar. If fighting is in Syria, title it "Syria Civil War" not "US Russia Syria Conflict"
- Drop peripheral actors: if multiple countries are listed but only one is where events occur, name only that one
- NEVER join unrelated topics with "and" — pick the single dominant theme
- NEVER use these vague filler words: tensions, escalation, escalate, escalates, developments, situation, multiple, various, activity, operations, concerns, shifts, movements, dynamics, implications, sparks, rift, row
- If events show active combat/strikes/casualties, call it a war, conflict, fighting, or strikes — never soften with "escalation" or "activity"
- NO technical acronyms: FIR, NOTAM, BGP, ASN, ICAO, ADS-B, SIGINT — use plain English
- For flight tracking: "[Region] Military Flights" or "[Country] Air Force Patrols"
- For fire/thermal: "[Location] Wildfires" or "[Location] Fire Clusters"
- For news clusters about a specific country: "[Country] [What's happening]"
- If entities list is empty or irrelevant, focus on the topics and event headlines for the core theme
- NEVER respond with "No relevant information" or any refusal
- Respond with ONLY the title text, no quotes, no explanation"#;

/// Build the user prompt for situation title generation.
#[allow(clippy::too_many_arguments)]
pub fn title_user(
    entities: &[String],
    topics: &[String],
    regions: &[String],
    event_titles: &[String],
    event_count: usize,
    source_count: usize,
    severity_dist: Option<&str>,
    event_type_breakdown: Option<&str>,
    fatality_count: Option<u32>,
    enrichment_summaries: &[String],
) -> String {
    let mut prompt = format!("Generate a concise situation title for this cluster of {event_count} events from {source_count} sources.\n\n");

    if let Some(dist) = severity_dist {
        prompt.push_str(&format!("Severity: {dist}\n"));
    }
    if let Some(breakdown) = event_type_breakdown {
        prompt.push_str(&format!("Event types: {breakdown}\n"));
    }
    if let Some(fatalities) = fatality_count {
        if fatalities > 0 {
            prompt.push_str(&format!("Total fatalities reported: {fatalities}\n"));
        }
    }
    if !entities.is_empty() {
        prompt.push_str(&format!("Key entities (pre-filtered for relevance): {}\n", entities.join(", ")));
    }
    if !topics.is_empty() {
        prompt.push_str(&format!("Topics: {}\n", topics.join(", ")));
    }
    if !regions.is_empty() {
        prompt.push_str(&format!("Regions: {}\n", regions.join(", ")));
    }
    if !enrichment_summaries.is_empty() {
        prompt.push_str("\nEnrichment summaries:\n");
        for s in enrichment_summaries.iter().take(3) {
            prompt.push_str(&format!("- {s}\n"));
        }
    }
    if !event_titles.is_empty() {
        prompt.push_str("\nSample event headlines (for context — your title should describe the OVERALL situation, not just these specific events):\n");
        // Take the LAST 5 titles (most recent) rather than the first 5
        let start = event_titles.len().saturating_sub(5);
        for t in event_titles[start..].iter().rev() {
            prompt.push_str(&format!("- {t}\n"));
        }
    }

    prompt
}

/// Event types that should be aggregated (summarized by count) rather than listed individually.
/// These are high-volume network telemetry that overwhelms the LLM if listed raw.
const AGGREGATE_EVENT_TYPES: &[&str] = &[
    "bgp_anomaly", "bgp_leak", "cert_issued", "shodan_banner", "shodan_count",
];

/// Build the user prompt for periodic analysis from current state.
pub fn analysis_user(
    situations: &[crate::types::SituationSummary],
    events: &[crate::types::EventSummary],
    tempo: &str,
) -> String {
    let mut prompt = format!("Current tempo: {tempo}\n\n");

    if !situations.is_empty() {
        // Limit to top 50 situations by severity and event count to keep prompt manageable
        let mut sorted: Vec<&crate::types::SituationSummary> = situations.iter().collect();
        sorted.sort_by(|a, b| {
            b.severity
                .rank()
                .cmp(&a.severity.rank())
                .then_with(|| b.event_count.cmp(&a.event_count))
        });
        let top = &sorted[..sorted.len().min(50)];

        prompt.push_str(&format!(
            "## Active Situations ({} of {} total)\n",
            top.len(),
            situations.len(),
        ));
        for s in top {
            prompt.push_str(&format!(
                "- [{}] {} | severity={} | region={} | events={} | sources={}\n",
                s.id,
                s.title,
                s.severity,
                s.region.as_deref().unwrap_or("global"),
                s.event_count,
                s.source_types.join(", "),
            ));
            if let Some(ref ctx) = s.web_context {
                prompt.push_str(&format!("  Web context: {ctx}\n"));
            }
        }
        prompt.push('\n');
    }

    if !events.is_empty() {
        // Separate high-signal events from network telemetry
        let mut signal_events = Vec::new();
        let mut aggregated: std::collections::HashMap<&str, (usize, Vec<String>)> = std::collections::HashMap::new();

        for e in events {
            let etype = e.event_type.as_str();
            if AGGREGATE_EVENT_TYPES.contains(&etype) {
                let entry = aggregated.entry(etype).or_insert_with(|| (0, Vec::new()));
                entry.0 += 1;
                // Keep a few sample titles for context (max 3)
                if entry.1.len() < 3 {
                    if let Some(t) = e.title.as_deref() {
                        entry.1.push(t.to_string());
                    }
                }
            } else {
                signal_events.push(e);
            }
        }

        // First: list high-signal events (cap at 40)
        if !signal_events.is_empty() {
            prompt.push_str("## Recent Events (last analysis window)\n");
            for e in signal_events.iter().take(40) {
                prompt.push_str(&format!(
                    "- {} | {} | sev={} | region={} | {}\n",
                    e.event_type,
                    e.title.as_deref().unwrap_or("(no title)"),
                    e.severity,
                    e.region.as_deref().unwrap_or("?"),
                    e.event_time.format("%H:%M:%S UTC"),
                ));
            }
        }

        // Then: summarize aggregated network telemetry
        if !aggregated.is_empty() {
            prompt.push_str("\n## Network Telemetry Summary (aggregated, low priority)\n");
            for (etype, (count, samples)) in &aggregated {
                prompt.push_str(&format!("- {etype}: {count} events"));
                if !samples.is_empty() {
                    prompt.push_str(&format!(" (e.g., {})", samples.join("; ")));
                }
                prompt.push('\n');
            }
            prompt.push_str("Note: These are routine network events. Only mention if clearly correlated with a kinetic situation.\n");
        }
    }

    prompt
}
