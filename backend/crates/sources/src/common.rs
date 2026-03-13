/// Map ISO 3166-1 alpha-2 country codes to broad region identifiers.
pub fn region_for_country(cc: &str) -> Option<&'static str> {
    match cc.to_uppercase().as_str() {
        "UA" | "RU" => Some("eastern-europe"),
        "IL" | "PS" | "SY" | "IR" | "LB" | "IQ" | "YE" | "BH" | "QA" | "KW" | "AE" | "JO"
        | "SA" | "OM" | "TR" | "CY" => Some("middle-east"),
        "SD" | "SO" | "ET" | "ER" | "DJ" | "LY" | "EG" => Some("africa"),
        "MM" | "TH" | "VN" | "LA" | "KH" | "PH" | "MY" | "SG" | "ID" => Some("southeast-asia"),
        "TW" | "CN" | "KR" | "KP" | "JP" => Some("east-asia"),
        _ => None,
    }
}

/// Map long country names to region identifiers.
pub fn region_for_country_name(name: &str) -> Option<&'static str> {
    match name {
        "Ukraine" | "Russia" => Some("eastern-europe"),
        "Iran" | "Israel" | "Syria" | "Lebanon" | "Yemen" | "Iraq" | "Jordan" | "Palestine"
        | "Saudi Arabia" | "Bahrain" | "Qatar" | "Kuwait" | "United Arab Emirates"
        | "Oman" | "Turkey" | "Cyprus" => Some("middle-east"),
        "Sudan" | "Somalia" | "Ethiopia" | "Eritrea" | "Djibouti" | "Libya" | "Egypt" => {
            Some("africa")
        }
        "Myanmar" | "Thailand" | "Vietnam" | "Philippines" => Some("southeast-asia"),
        "Taiwan" | "China" | "South Korea" | "North Korea" | "Japan" => Some("east-asia"),
        _ => None,
    }
}

/// Infer a region code from latitude/longitude using broad bounding boxes.
/// Returns the first matching region. Regions overlap slightly to reduce gaps.
pub fn region_from_coords(lat: f64, lon: f64) -> Option<&'static str> {
    // Middle East: roughly 12°N-42°N, 25°E-63°E
    if lat >= 12.0 && lat <= 42.0 && lon >= 25.0 && lon <= 63.0 {
        return Some("middle-east");
    }
    // Eastern Europe (Ukraine/Russia/Belarus): 44°N-60°N, 22°E-50°E
    if lat >= 44.0 && lat <= 60.0 && lon >= 22.0 && lon <= 50.0 {
        return Some("eastern-europe");
    }
    // Western Europe: 35°N-72°N, -12°W-22°E
    if lat >= 35.0 && lat <= 72.0 && lon >= -12.0 && lon <= 22.0 {
        return Some("western-europe");
    }
    // East Asia: 18°N-55°N, 100°E-150°E
    if lat >= 18.0 && lat <= 55.0 && lon >= 100.0 && lon <= 150.0 {
        return Some("east-asia");
    }
    // Southeast Asia: -12°S-25°N, 90°E-140°E
    if lat >= -12.0 && lat <= 25.0 && lon >= 90.0 && lon <= 140.0 {
        return Some("southeast-asia");
    }
    // South Asia: 5°N-38°N, 60°E-100°E
    if lat >= 5.0 && lat <= 38.0 && lon >= 60.0 && lon <= 100.0 {
        return Some("south-asia");
    }
    // Africa: -35°S-37°N, -18°W-52°E (excluding Middle East overlap)
    if lat >= -35.0 && lat <= 37.0 && lon >= -18.0 && lon <= 52.0 && lat < 12.0 {
        return Some("africa");
    }
    // North Africa: 15°N-37°N, -18°W-25°E
    if lat >= 15.0 && lat <= 37.0 && lon >= -18.0 && lon <= 25.0 {
        return Some("north-africa");
    }
    // North America: 15°N-85°N, -170°W-52°W
    if lat >= 15.0 && lat <= 85.0 && lon >= -170.0 && lon <= -52.0 {
        return Some("north-america");
    }
    // South America: -56°S-15°N, -82°W-34°W
    if lat >= -56.0 && lat <= 15.0 && lon >= -82.0 && lon <= -34.0 {
        return Some("south-america");
    }
    // Central America / Caribbean: 7°N-23°N, -92°W-59°W
    if lat >= 7.0 && lat <= 23.0 && lon >= -92.0 && lon <= -59.0 {
        return Some("central-america");
    }
    // Oceania: -50°S-0°, 110°E-180°E
    if lat >= -50.0 && lat <= 0.0 && lon >= 110.0 && lon <= 180.0 {
        return Some("oceania");
    }
    // Central Asia: 35°N-55°N, 50°E-90°E
    if lat >= 35.0 && lat <= 55.0 && lon >= 50.0 && lon <= 90.0 {
        return Some("central-asia");
    }
    // Arctic: above 65°N
    if lat >= 65.0 {
        return Some("arctic");
    }
    // Catch-all: events that don't match any named region
    Some("global")
}

/// Approximate center coordinates (lat, lon) for region codes.
/// Used as fallback when events/situations lack explicit coordinates.
pub fn region_center(region: &str) -> Option<(f64, f64)> {
    match region.to_lowercase().as_str() {
        "middle-east" => Some((27.0, 44.0)),
        "eastern-europe" => Some((48.5, 31.0)),
        "western-europe" => Some((48.0, 2.0)),
        "africa" | "sub-saharan-africa" => Some((8.0, 25.0)),
        "north-africa" => Some((28.0, 15.0)),
        "southeast-asia" => Some((15.0, 105.0)),
        "east-asia" => Some((35.0, 120.0)),
        "south-asia" => Some((25.0, 78.0)),
        "central-asia" => Some((42.0, 65.0)),
        "north-america" => Some((40.0, -100.0)),
        "south-america" => Some((-15.0, -55.0)),
        "central-america" | "caribbean" => Some((15.0, -80.0)),
        "oceania" => Some((-25.0, 135.0)),
        "arctic" => Some((75.0, 0.0)),
        _ => None,
    }
}

/// Approximate center coordinates (lat, lon) for ISO 3166-1 alpha-2 country codes.
pub fn country_center(cc: &str) -> Option<(f64, f64)> {
    match cc.to_uppercase().as_str() {
        "GB" | "UK" => Some((54.0, -2.0)),
        "US" => Some((39.0, -98.0)),
        "UA" => Some((48.5, 31.2)),
        "RU" => Some((56.0, 38.0)),
        "IL" => Some((31.5, 34.8)),
        "PS" => Some((31.9, 35.2)),
        "SY" => Some((35.0, 38.0)),
        "IR" => Some((32.4, 53.7)),
        "IQ" => Some((33.0, 44.0)),
        "LB" => Some((33.9, 35.8)),
        "YE" => Some((15.6, 48.5)),
        "SA" => Some((24.7, 45.1)),
        "TR" => Some((39.0, 35.2)),
        "EG" => Some((26.8, 30.8)),
        "SD" => Some((15.5, 32.5)),
        "SO" => Some((5.0, 46.0)),
        "ET" => Some((9.0, 38.7)),
        "LY" => Some((27.0, 17.0)),
        "CN" => Some((35.0, 105.0)),
        "TW" => Some((23.7, 121.0)),
        "KR" => Some((36.5, 128.0)),
        "KP" => Some((40.0, 127.0)),
        "JP" => Some((36.2, 138.3)),
        "MM" => Some((19.8, 96.0)),
        "PH" => Some((12.9, 121.8)),
        "CD" => Some((-4.0, 21.8)),    // DRC
        "ML" => Some((17.6, -4.0)),    // Mali
        "NE" => Some((17.6, 8.1)),     // Niger
        "BF" => Some((12.4, -1.6)),    // Burkina Faso
        "NG" => Some((9.1, 7.5)),      // Nigeria
        "DE" => Some((51.2, 10.5)),
        "FR" => Some((46.6, 2.2)),
        "PL" => Some((52.0, 19.0)),
        "NO" => Some((60.5, 8.5)),
        "SE" => Some((62.0, 15.0)),
        "FI" => Some((64.0, 26.0)),
        "IN" => Some((20.6, 78.9)),
        "PK" => Some((30.4, 69.3)),
        "AF" => Some((33.9, 67.7)),
        // Humanitarian crisis countries
        "HT" => Some((19.0, -72.4)),    // Haiti
        "BD" => Some((23.7, 90.4)),     // Bangladesh
        "MZ" => Some((-18.7, 35.5)),    // Mozambique
        "SS" => Some((6.9, 31.3)),      // South Sudan
        "CF" => Some((6.6, 20.9)),      // Central African Republic
        "TD" => Some((15.5, 18.7)),     // Chad
        "CM" => Some((7.4, 12.4)),      // Cameroon
        "KE" => Some((-0.0, 37.9)),     // Kenya
        "UG" => Some((1.4, 32.3)),      // Uganda
        "RW" => Some((-1.9, 29.9)),     // Rwanda
        "BI" => Some((-3.4, 29.9)),     // Burundi
        "CO" => Some((4.6, -74.1)),     // Colombia
        "VE" => Some((6.4, -66.6)),     // Venezuela
        "NP" => Some((28.4, 84.1)),     // Nepal
        "LK" => Some((7.9, 80.8)),      // Sri Lanka
        "ID" => Some((-0.8, 113.9)),    // Indonesia
        "MG" => Some((-18.8, 46.9)),    // Madagascar
        "MW" => Some((-13.3, 34.3)),    // Malawi
        "ZW" => Some((-20.0, 30.0)),    // Zimbabwe
        "SL" => Some((8.5, -11.8)),     // Sierra Leone
        "LR" => Some((6.4, -9.4)),      // Liberia
        "GN" => Some((9.9, -9.7)),      // Guinea
        "DJ" => Some((11.6, 43.1)),     // Djibouti
        "ER" => Some((15.2, 39.8)),     // Eritrea
        "JO" => Some((31.2, 36.6)),     // Jordan
        _ => None,
    }
}

/// Approximate center coordinates (lat, lon) for country names (case-insensitive).
/// Broader coverage than `country_center()` which uses ISO 2-letter codes.
pub fn country_center_for_name(name: &str) -> Option<(f64, f64)> {
    match name.to_lowercase().as_str() {
        "iraq" => Some((33.3, 44.4)),
        "syria" => Some((34.8, 38.9)),
        "yemen" => Some((15.5, 48.5)),
        "iran" => Some((35.7, 51.4)),
        "lebanon" => Some((33.9, 35.5)),
        "israel" => Some((31.5, 34.8)),
        "palestine" | "palestinian territories" | "gaza" | "west bank" => Some((31.9, 35.2)),
        "ukraine" => Some((48.4, 31.2)),
        "russia" => Some((56.0, 38.0)),
        "turkey" | "türkiye" => Some((39.0, 35.2)),
        "saudi arabia" => Some((24.7, 45.1)),
        "egypt" => Some((26.8, 30.8)),
        "jordan" => Some((31.9, 36.0)),
        "kuwait" => Some((29.4, 47.9)),
        "bahrain" => Some((26.1, 50.6)),
        "qatar" => Some((25.3, 51.2)),
        "united arab emirates" | "uae" => Some((24.5, 54.7)),
        "oman" => Some((23.6, 58.5)),
        "cyprus" => Some((35.1, 33.4)),
        "sudan" => Some((15.5, 32.5)),
        "south sudan" => Some((6.9, 31.3)),
        "somalia" => Some((5.0, 46.0)),
        "ethiopia" => Some((9.0, 38.7)),
        "eritrea" => Some((15.2, 39.8)),
        "djibouti" => Some((11.6, 43.1)),
        "libya" => Some((27.0, 17.0)),
        "china" => Some((35.0, 105.0)),
        "taiwan" => Some((23.7, 121.0)),
        "south korea" => Some((36.5, 128.0)),
        "north korea" => Some((40.0, 127.0)),
        "japan" => Some((36.2, 138.3)),
        "myanmar" | "burma" => Some((19.8, 96.0)),
        "thailand" => Some((15.9, 100.5)),
        "vietnam" => Some((14.1, 108.3)),
        "philippines" => Some((12.9, 121.8)),
        "indonesia" => Some((-0.8, 113.9)),
        "malaysia" => Some((4.2, 101.9)),
        "singapore" => Some((1.35, 103.8)),
        "india" => Some((20.6, 78.9)),
        "pakistan" => Some((30.4, 69.3)),
        "afghanistan" => Some((33.9, 67.7)),
        "united states" | "usa" | "united states of america" => Some((39.0, -98.0)),
        "united kingdom" | "uk" | "britain" | "great britain" => Some((54.0, -2.0)),
        "germany" => Some((51.2, 10.5)),
        "france" => Some((46.6, 2.2)),
        "poland" => Some((52.0, 19.0)),
        "nigeria" => Some((9.1, 7.5)),
        "mali" => Some((17.6, -4.0)),
        "niger" => Some((17.6, 8.1)),
        "burkina faso" => Some((12.4, -1.6)),
        "democratic republic of the congo" | "drc" | "congo" => Some((-4.0, 21.8)),
        "mozambique" => Some((-18.7, 35.5)),
        "cameroon" => Some((7.4, 12.4)),
        "chad" => Some((15.5, 18.7)),
        "central african republic" => Some((6.6, 20.9)),
        "kenya" => Some((-0.02, 37.9)),
        "tunisia" => Some((34.0, 9.5)),
        "algeria" => Some((28.0, 1.7)),
        "morocco" => Some((31.8, -7.1)),
        "mexico" => Some((23.6, -102.5)),
        "colombia" => Some((4.6, -74.1)),
        "venezuela" => Some((6.4, -66.6)),
        "brazil" => Some((-14.2, -51.9)),
        "argentina" => Some((-38.4, -63.6)),
        // Additional humanitarian crisis countries
        "haiti" => Some((19.0, -72.4)),
        "bangladesh" => Some((23.7, 90.4)),
        "nepal" => Some((28.4, 84.1)),
        "sri lanka" => Some((7.9, 80.8)),
        "madagascar" => Some((-18.8, 46.9)),
        "malawi" => Some((-13.3, 34.3)),
        "zimbabwe" => Some((-20.0, 30.0)),
        "uganda" => Some((1.4, 32.3)),
        "rwanda" => Some((-1.9, 29.9)),
        "burundi" => Some((-3.4, 29.9)),
        "sierra leone" => Some((8.5, -11.8)),
        "liberia" => Some((6.4, -9.4)),
        "guinea" => Some((9.9, -9.7)),
        "honduras" => Some((15.2, -86.2)),
        "guatemala" => Some((15.8, -90.2)),
        "el salvador" => Some((13.8, -88.9)),
        "peru" => Some((-9.2, -75.0)),
        "bolivia" => Some((-16.3, -63.6)),
        "cambodia" => Some((12.6, 104.9)),
        "laos" | "lao people's democratic republic" => Some((19.9, 102.5)),
        "papua new guinea" => Some((-6.3, 147.2)),
        "fiji" => Some((-17.7, 178.1)),
        "vanuatu" => Some((-15.4, 166.9)),
        "samoa" => Some((-13.8, -172.0)),
        "tonga" => Some((-21.2, -175.2)),
        "occupied palestinian territory" => Some((31.9, 35.2)),
        "syrian arab republic" => Some((34.8, 38.9)),
        "iran (islamic republic of)" => Some((35.7, 51.4)),
        "democratic people's republic of korea" => Some((40.0, 127.0)),
        "republic of korea" => Some((36.5, 128.0)),
        _ => None,
    }
}

/// Geocode a location entity name to approximate (lat, lon) coordinates.
///
/// Covers major cities, governorates, and sub-national regions in intelligence
/// coverage areas. Falls back to `country_center_for_name()` for country names.
/// Case-insensitive lookup. Returns `None` for unknown locations.
pub fn geocode_entity(name: &str) -> Option<(f64, f64)> {
    let lower = name.to_lowercase();
    // Strip common suffixes to normalize: "Babil Governorate" → "babil"
    let normalized = lower
        .trim()
        .trim_end_matches(" governorate")
        .trim_end_matches(" province")
        .trim_end_matches(" oblast")
        .trim_end_matches(" region")
        .trim_end_matches(" district")
        .trim_end_matches(" city")
        .trim_end_matches(" strip")
        .trim();

    match normalized {
        // ── Middle East: Iraq ──────────────────────────────────────────
        "baghdad" => Some((33.31, 44.37)),
        "basra" | "basrah" | "al-basrah" => Some((30.51, 47.81)),
        "mosul" | "al-mawsil" | "nineveh" => Some((36.34, 43.12)),
        "erbil" | "arbil" | "hawler" => Some((36.19, 44.01)),
        "kirkuk" => Some((35.47, 44.39)),
        "najaf" | "an-najaf" => Some((32.00, 44.34)),
        "karbala" => Some((32.62, 44.02)),
        "babil" | "hillah" | "al-hillah" => Some((32.48, 44.42)),
        "sulaymaniyah" | "suleimaniya" => Some((35.56, 45.44)),
        "diyala" | "baqubah" => Some((33.75, 44.66)),
        "anbar" | "ramadi" | "al-anbar" => Some((33.43, 43.31)),
        "tikrit" | "saladin" | "salah ad-din" => Some((34.60, 43.68)),
        "fallujah" => Some((33.35, 43.78)),
        "samarra" => Some((34.20, 43.87)),
        "duhok" | "dahuk" => Some((36.87, 43.00)),

        // ── Middle East: Syria ─────────────────────────────────────────
        "damascus" | "dimashq" => Some((33.51, 36.29)),
        "aleppo" | "halab" => Some((36.20, 37.15)),
        "homs" | "hims" => Some((34.73, 36.71)),
        "hama" | "hamah" => Some((35.13, 36.76)),
        "latakia" | "al-ladhiqiyah" => Some((35.52, 35.79)),
        "deir ez-zor" | "deir ezzor" | "deir al-zor" => Some((35.34, 40.14)),
        "raqqa" | "ar-raqqah" => Some((35.95, 39.01)),
        "idlib" => Some((35.93, 36.63)),
        "daraa" | "deraa" => Some((32.62, 36.10)),
        "qamishli" | "qamishlo" => Some((37.05, 41.22)),
        "tartus" | "tartous" => Some((34.89, 35.89)),
        "al-hasakah" | "hasakah" | "hasaka" => Some((36.50, 40.75)),
        "palmyra" | "tadmur" => Some((34.56, 38.26)),

        // ── Middle East: Iran ──────────────────────────────────────────
        "tehran" | "teheran" => Some((35.69, 51.39)),
        "isfahan" | "esfahan" => Some((32.65, 51.68)),
        "tabriz" => Some((38.08, 46.29)),
        "shiraz" => Some((29.59, 52.58)),
        "mashhad" | "mashad" => Some((36.30, 59.60)),
        "natanz" => Some((33.51, 51.92)),
        "fordow" | "fordo" => Some((34.88, 51.59)),
        "bushehr" | "bandar bushehr" => Some((28.97, 50.84)),
        "bandar abbas" => Some((27.19, 56.27)),
        "ahvaz" | "ahwaz" => Some((31.32, 48.67)),
        "qom" => Some((34.64, 50.88)),
        "kerman" => Some((30.28, 57.07)),
        "kermanshah" => Some((34.31, 47.07)),
        "chabahar" => Some((25.30, 60.64)),
        "hormuz" | "strait of hormuz" => Some((26.59, 56.28)),
        "kharg island" => Some((29.24, 50.33)),

        // ── Middle East: Lebanon ───────────────────────────────────────
        "beirut" => Some((33.89, 35.50)),
        "tripoli" => Some((34.43, 35.84)),
        "sidon" | "saida" => Some((33.56, 35.37)),
        "tyre" | "sour" => Some((33.27, 35.20)),
        "baalbek" | "baalbeck" => Some((34.01, 36.21)),
        "nabatieh" | "nabatiyeh" => Some((33.38, 35.48)),
        "bekaa" | "beqaa" | "bekaa valley" => Some((33.85, 35.90)),

        // ── Middle East: Yemen ─────────────────────────────────────────
        "sanaa" | "sana'a" | "sana" => Some((15.37, 44.19)),
        "aden" => Some((12.80, 45.04)),
        "hodeidah" | "hodeida" | "al hudaydah" => Some((14.80, 42.95)),
        "taiz" | "ta'izz" => Some((13.58, 44.02)),
        "marib" | "ma'rib" => Some((15.46, 45.32)),
        "mukalla" | "al mukalla" => Some((14.54, 49.12)),
        "saada" | "sa'dah" => Some((16.94, 43.76)),

        // ── Middle East: Israel / Palestine ────────────────────────────
        "jerusalem" | "al-quds" => Some((31.77, 35.23)),
        "tel aviv" | "tel-aviv" => Some((32.07, 34.77)),
        "haifa" => Some((32.79, 34.99)),
        "gaza" => Some((31.50, 34.47)),
        "rafah" => Some((31.28, 34.24)),
        "khan yunis" | "khan younis" => Some((31.34, 34.30)),
        "nablus" => Some((32.22, 35.25)),
        "hebron" | "al-khalil" => Some((31.53, 35.10)),
        "ramallah" => Some((31.90, 35.20)),
        "beersheba" | "beer sheva" | "be'er sheva" => Some((31.25, 34.79)),
        "ashkelon" => Some((31.67, 34.57)),
        "ashdod" => Some((31.80, 34.65)),
        "sderot" => Some((31.52, 34.60)),
        "netanya" => Some((32.33, 34.86)),
        "jenin" => Some((32.46, 35.30)),
        "tulkarm" | "tulkarem" => Some((32.31, 35.03)),
        "golan" | "golan heights" => Some((33.00, 35.75)),
        "negev" => Some((30.85, 34.78)),
        "dimona" => Some((31.07, 35.03)),

        // ── Middle East: Saudi Arabia ──────────────────────────────────
        "riyadh" => Some((24.69, 46.72)),
        "jeddah" | "jidda" => Some((21.54, 39.17)),
        "mecca" | "makkah" => Some((21.39, 39.86)),
        "medina" | "madinah" => Some((24.47, 39.61)),
        "dammam" => Some((26.43, 50.10)),
        "dhahran" => Some((26.27, 50.15)),
        "abha" => Some((18.22, 42.50)),
        "jizan" | "jazan" => Some((16.89, 42.55)),
        "tabuk" => Some((28.38, 36.57)),
        "neom" => Some((27.95, 35.29)),

        // ── Middle East: Jordan ────────────────────────────────────────
        "amman" => Some((31.95, 35.93)),
        "zarqa" => Some((32.07, 36.09)),
        "irbid" => Some((32.56, 35.85)),
        "aqaba" => Some((29.53, 35.01)),

        // ── Middle East: Turkey ────────────────────────────────────────
        "ankara" => Some((39.93, 32.86)),
        "istanbul" => Some((41.01, 28.98)),
        "izmir" => Some((38.42, 27.14)),
        "antalya" => Some((36.90, 30.69)),
        "incirlik" => Some((37.00, 35.43)),
        "diyarbakir" | "diyarbakır" => Some((37.91, 40.22)),
        "gaziantep" => Some((37.07, 37.38)),
        "hatay" => Some((36.40, 36.35)),
        "adana" => Some((37.00, 35.32)),

        // ── Eastern Europe: Ukraine ────────────────────────────────────
        "kyiv" | "kiev" => Some((50.45, 30.52)),
        "kharkiv" | "kharkov" => Some((49.99, 36.23)),
        "odesa" | "odessa" => Some((46.48, 30.73)),
        "dnipro" | "dnepropetrovsk" => Some((48.46, 35.05)),
        "lviv" | "lvov" => Some((49.84, 24.03)),
        "zaporizhzhia" | "zaporozhye" | "zaporizhia" => Some((47.84, 35.14)),
        "mariupol" => Some((47.10, 37.55)),
        "kherson" => Some((46.63, 32.62)),
        "mykolaiv" | "nikolaev" => Some((46.97, 32.00)),
        "donetsk" => Some((48.00, 37.81)),
        "luhansk" | "lugansk" => Some((48.57, 39.31)),
        "sumy" => Some((50.91, 34.80)),
        "chernihiv" | "chernigov" => Some((51.49, 31.29)),
        "poltava" => Some((49.59, 34.55)),
        "vinnytsia" | "vinnitsa" => Some((49.23, 28.47)),
        "zhytomyr" | "zhitomir" => Some((50.25, 28.66)),
        "bakhmut" | "artyomovsk" => Some((48.59, 38.00)),
        "avdiivka" | "avdeevka" => Some((48.14, 37.74)),
        "kramatorsk" => Some((48.74, 37.56)),
        "izium" | "izyum" => Some((49.21, 37.26)),
        "melitopol" => Some((46.84, 35.37)),
        "sevastopol" => Some((44.62, 33.52)),
        "simferopol" => Some((44.95, 34.10)),
        "crimea" | "crimean peninsula" => Some((45.30, 34.08)),
        "donbas" | "donbass" => Some((48.30, 38.00)),

        // ── Eastern Europe: Russia ─────────────────────────────────────
        "moscow" | "moskva" => Some((55.76, 37.62)),
        "st. petersburg" | "saint petersburg" | "st petersburg" | "leningrad" => {
            Some((59.93, 30.32))
        }
        "rostov-on-don" | "rostov" => Some((47.24, 39.71)),
        "volgograd" | "stalingrad" => Some((48.71, 44.51)),
        "voronezh" => Some((51.67, 39.21)),
        "belgorod" => Some((50.60, 36.60)),
        "kursk" => Some((51.73, 36.19)),
        "bryansk" => Some((53.24, 34.37)),
        "murmansk" => Some((68.97, 33.07)),
        "kaliningrad" => Some((54.71, 20.51)),
        "novosibirsk" => Some((55.01, 82.92)),
        "vladivostok" => Some((43.12, 131.89)),
        "severo-kurilsk" | "kuril islands" => Some((50.68, 156.12)),
        "chechnya" | "grozny" => Some((43.32, 45.69)),
        "dagestan" | "makhachkala" => Some((42.98, 47.50)),

        // ── Eastern Europe: Belarus ────────────────────────────────────
        "minsk" => Some((53.90, 27.57)),

        // ── Eastern Europe: Moldova ────────────────────────────────────
        "chisinau" | "kishinev" => Some((47.01, 28.86)),
        "tiraspol" | "transnistria" => Some((46.84, 29.64)),

        // ── Africa: Sudan ──────────────────────────────────────────────
        "khartoum" => Some((15.59, 32.53)),
        "omdurman" => Some((15.64, 32.48)),
        "port sudan" => Some((19.62, 37.22)),
        "el-fasher" | "al-fashir" | "el fasher" => Some((13.63, 25.35)),
        "darfur" => Some((13.00, 25.00)),
        "wad madani" | "wad medani" => Some((14.40, 33.52)),

        // ── Africa: Somalia ────────────────────────────────────────────
        "mogadishu" | "muqdisho" => Some((2.05, 45.32)),
        "hargeisa" => Some((9.56, 44.06)),
        "kismaayo" | "kismayo" => Some((-0.35, 42.54)),

        // ── Africa: Ethiopia ───────────────────────────────────────────
        "addis ababa" => Some((9.02, 38.75)),
        "mekelle" | "mek'ele" => Some((13.50, 39.47)),
        "tigray" => Some((13.50, 39.50)),

        // ── Africa: Libya ──────────────────────────────────────────────
        // Note: bare "tripoli" maps to Tripoli, Lebanon above (first match wins).
        // Use qualified names for Tripoli, Libya:
        "tripoli libya" | "tarabulus" => Some((32.90, 13.18)),
        "benghazi" => Some((32.12, 20.09)),
        "misrata" | "misurata" => Some((32.38, 15.09)),
        "sirte" | "sirt" => Some((31.21, 16.59)),
        "tobruk" => Some((32.08, 23.96)),

        // ── Africa: Nigeria ────────────────────────────────────────────
        "abuja" => Some((9.06, 7.49)),
        "lagos" => Some((6.52, 3.38)),
        "maiduguri" | "borno" => Some((11.85, 13.16)),

        // ── Africa: DRC ───────────────────────────────────────────────
        "kinshasa" => Some((-4.44, 15.27)),
        "goma" | "north kivu" => Some((-1.68, 29.23)),
        "bukavu" | "south kivu" => Some((-2.51, 28.86)),

        // ── Africa: Other ──────────────────────────────────────────────
        "nairobi" => Some((-1.29, 36.82)),
        "cairo" => Some((30.04, 31.24)),
        "alexandria" => Some((31.20, 29.92)),
        "tunis" => Some((36.81, 10.17)),
        "algiers" => Some((36.75, 3.04)),
        "cape town" => Some((-33.92, 18.42)),
        "johannesburg" => Some((-26.20, 28.05)),

        // ── East Asia ──────────────────────────────────────────────────
        "taipei" => Some((25.03, 121.57)),
        "beijing" | "peking" => Some((39.90, 116.40)),
        "shanghai" => Some((31.23, 121.47)),
        "hong kong" => Some((22.32, 114.17)),
        "seoul" => Some((37.57, 126.98)),
        "pyongyang" => Some((39.02, 125.74)),
        "tokyo" => Some((35.68, 139.69)),
        "okinawa" => Some((26.50, 127.94)),
        "lhasa" | "tibet" => Some((29.65, 91.17)),
        "urumqi" | "xinjiang" => Some((43.83, 87.62)),

        // ── South/Southeast Asia ───────────────────────────────────────
        "manila" => Some((14.60, 120.98)),
        "kabul" => Some((34.53, 69.17)),
        "islamabad" => Some((33.69, 73.04)),
        "karachi" => Some((24.86, 67.01)),
        "new delhi" | "delhi" => Some((28.61, 77.21)),
        "mumbai" | "bombay" => Some((19.08, 72.88)),
        "dhaka" | "dacca" => Some((23.81, 90.41)),
        "naypyidaw" | "nay pyi taw" => Some((19.76, 96.07)),
        "yangon" | "rangoon" => Some((16.87, 96.20)),
        "bangkok" => Some((13.76, 100.50)),
        "hanoi" => Some((21.03, 105.85)),
        "ho chi minh" | "saigon" => Some((10.82, 106.63)),

        // ── Europe: Other ──────────────────────────────────────────────
        "london" => Some((51.51, -0.13)),
        "paris" => Some((48.86, 2.35)),
        "berlin" => Some((52.52, 13.41)),
        "brussels" => Some((50.85, 4.35)),
        "the hague" | "hague" => Some((52.07, 4.30)),
        "warsaw" => Some((52.23, 21.01)),
        "oslo" => Some((59.91, 10.75)),
        "stockholm" => Some((59.33, 18.07)),
        "helsinki" => Some((60.17, 24.94)),
        "tallinn" => Some((59.44, 24.75)),
        "riga" => Some((56.95, 24.11)),
        "vilnius" => Some((54.69, 25.28)),
        "bucharest" => Some((44.43, 26.10)),

        // ── Americas ──────────────────────────────────────────────────
        "washington" | "washington dc" | "washington d.c." => Some((38.91, -77.04)),
        "new york" => Some((40.71, -74.01)),
        "bogota" | "bogotá" => Some((4.71, -74.07)),
        "caracas" => Some((10.48, -66.90)),
        "mexico city" => Some((19.43, -99.13)),
        "havana" | "la habana" => Some((23.11, -82.37)),

        // ── Waterways / Strategic Points ───────────────────────────────
        "bab el-mandeb" | "bab al-mandab" | "bab el mandeb" => Some((12.58, 43.33)),
        "suez canal" | "suez" => Some((30.46, 32.35)),
        "red sea" => Some((20.0, 38.0)),
        "gulf of aden" => Some((12.5, 47.0)),
        "persian gulf" | "arabian gulf" => Some((26.0, 52.0)),
        "taiwan strait" | "formosa strait" => Some((24.5, 119.5)),
        "south china sea" => Some((12.0, 114.0)),
        "black sea" => Some((43.0, 35.0)),
        "sea of azov" | "azov sea" => Some((46.0, 36.8)),
        "east china sea" => Some((30.0, 126.0)),

        // ── Fall through to country name lookup ────────────────────────
        _ => country_center_for_name(normalized),
    }
}

/// Check if the given lat/lon matches a known region centroid (within 0.01 degrees).
/// Region centroids are generic fallback coordinates that don't represent actual event locations.
pub fn is_region_centroid(lat: f64, lon: f64) -> bool {
    const EPSILON: f64 = 0.05;
    let centroids = [
        (27.0, 44.0),    // middle-east
        (48.5, 31.0),    // eastern-europe
        (48.0, 2.0),     // western-europe
        (8.0, 25.0),     // africa / sub-saharan-africa
        (28.0, 15.0),    // north-africa
        (15.0, 105.0),   // southeast-asia
        (35.0, 120.0),   // east-asia
        (25.0, 78.0),    // south-asia
        (42.0, 65.0),    // central-asia
        (40.0, -100.0),  // north-america
        (-15.0, -55.0),  // south-america
        (15.0, -80.0),   // central-america / caribbean
        (-25.0, 135.0),  // oceania
        (75.0, 0.0),     // arctic
    ];
    centroids.iter().any(|(clat, clon)| {
        (lat - clat).abs() < EPSILON && (lon - clon).abs() < EPSILON
    })
}

/// Try to parse a JSON value as f64 (handles both number and string).
pub fn json_as_f64(val: &serde_json::Value) -> Option<f64> {
    val.as_f64()
        .or_else(|| val.as_str().and_then(|s| s.parse::<f64>().ok()))
}

/// URL-encode a string (RFC 3986 unreserved characters pass through).
pub fn urlencode(input: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => {
                let _ = write!(out, "%{:02X}", byte);
            }
        }
    }
    out
}

/// Return 2-letter country code for known military callsign prefixes.
///
/// Prefixes are checked longest-first to avoid false matches (e.g. "RAF"
/// must not shadow a hypothetical longer prefix starting with "RAF").
pub fn callsign_country(callsign: &str) -> Option<&'static str> {
    let cs = callsign.to_uppercase();
    // Longest prefixes first (6-letter)
    if cs.starts_with("SENTRY") {
        return Some("US"); // AWACS
    }
    // 5-letter prefixes
    if cs.starts_with("REACH") || cs.starts_with("ETHYL") || cs.starts_with("HOMER")
        || cs.starts_with("GORDO") || cs.starts_with("TOPPS") || cs.starts_with("BISON")
        || cs.starts_with("MOOSE") {
        return Some("US");
    }
    if cs.starts_with("FORTE") || cs.starts_with("JAKE") {
        return Some("US"); // Recon platforms
    }
    if cs.starts_with("ASCOT") {
        return Some("GB");
    }
    // 4-letter prefixes
    if cs.starts_with("LAGR") || cs.starts_with("EVAC") || cs.starts_with("HUNT")
        || cs.starts_with("BFLO") {
        return Some("US");
    }
    if cs.starts_with("DUKE") || cs.starts_with("NATO") || cs.starts_with("NCHO") {
        return Some("NATO");
    }
    // 3-letter prefixes
    if cs.starts_with("RCH") || cs.starts_with("RFF")
        || cs.starts_with("PAT") || cs.starts_with("CDT") {
        return Some("US");
    }
    if cs.starts_with("RSD") {
        return Some("RU"); // Rossiya Special Flight Squadron
    }
    if cs.starts_with("RAF") {
        return Some("GB");
    }
    if cs.starts_with("FAF") || cs.starts_with("CTM") || cs.starts_with("RFR") {
        return Some("FR");
    }
    if cs.starts_with("GAF") {
        return Some("DE");
    }
    if cs.starts_with("IAF") {
        return Some("IL");
    }
    if cs.starts_with("IAM") {
        return Some("IT");
    }
    if cs.starts_with("BAF") {
        return Some("BE");
    }
    if cs.starts_with("CFC") || cs.starts_with("CAN") {
        return Some("CA");
    }
    if cs.starts_with("RRR") {
        return Some("AU");
    }
    if cs.starts_with("SWS") {
        return Some("SE");
    }
    if cs.starts_with("NOR") {
        return Some("NO");
    }
    if cs.starts_with("TUR") || cs.starts_with("THK") {
        return Some("TR");
    }
    if cs.starts_with("KNG") {
        return Some("SA"); // Saudi Royal
    }
    if cs.starts_with("PLF") || cs.starts_with("PAF") {
        return Some("PL");
    }
    if cs.starts_with("HEL") {
        return Some("GR"); // Hellenic AF
    }
    if cs.starts_with("HAF") {
        return Some("GR");
    }
    if cs.starts_with("JAF") {
        return Some("JO"); // Jordanian AF
    }
    if cs.starts_with("EGF") {
        return Some("EG");
    }
    None
}

/// Return 2-letter country code from an ICAO hex (Mode-S) address.
///
/// ICAO allocates hex ranges to each country. We cover the most common
/// military-relevant nations. The hex is parsed as a u32 and matched
/// against known allocation blocks.
pub fn icao_hex_country(hex: &str) -> Option<&'static str> {
    let val = u32::from_str_radix(hex.trim(), 16).ok()?;
    match val {
        // United States (A00000-ADF7C7)
        0xA00000..=0xADF7C7 => Some("US"),
        // United Kingdom (400000-43FFFF)
        0x400000..=0x43FFFF => Some("GB"),
        // Germany (3C0000-3FFFFF)
        0x3C0000..=0x3FFFFF => Some("DE"),
        // France (380000-3BFFFF)
        0x380000..=0x3BFFFF => Some("FR"),
        // Italy (300000-33FFFF)
        0x300000..=0x33FFFF => Some("IT"),
        // Spain (340000-37FFFF)
        0x340000..=0x37FFFF => Some("ES"),
        // Russia (140000-17FFFF)
        0x140000..=0x17FFFF => Some("RU"),
        // China (780000-7BFFFF)
        0x780000..=0x7BFFFF => Some("CN"),
        // Israel (738000-73FFFF)
        0x738000..=0x73FFFF => Some("IL"),
        // Iran (730000-737FFF)
        0x730000..=0x737FFF => Some("IR"),
        // Turkey (440000-447FFF)
        0x440000..=0x447FFF => Some("TR"),
        // Ukraine (E00000-E3FFFF -- actually 508000-50FFFF in some allocations)
        0x508000..=0x50FFFF => Some("UA"),
        // Canada (C00000-C3FFFF)
        0xC00000..=0xC3FFFF => Some("CA"),
        // Australia (7C0000-7FFFFF)
        0x7C0000..=0x7FFFFF => Some("AU"),
        // New Zealand (C80000-C87FFF)
        0xC80000..=0xC87FFF => Some("NZ"),
        // Japan (840000-87FFFF)
        0x840000..=0x87FFFF => Some("JP"),
        // South Korea (710000-717FFF)
        0x710000..=0x717FFF => Some("KR"),
        // India (800000-83FFFF)
        0x800000..=0x83FFFF => Some("IN"),
        // Saudi Arabia (700000-70FFFF)
        0x700000..=0x70FFFF => Some("SA"),
        // Netherlands (480000-487FFF)
        0x480000..=0x487FFF => Some("NL"),
        // Belgium (448000-44FFFF)
        0x448000..=0x44FFFF => Some("BE"),
        // Norway (478000-47FFFF)
        0x478000..=0x47FFFF => Some("NO"),
        // Sweden (4A0000-4A7FFF)
        0x4A0000..=0x4A7FFF => Some("SE"),
        // Poland (488000-48FFFF)
        0x488000..=0x48FFFF => Some("PL"),
        // Greece (468000-46FFFF)
        0x468000..=0x46FFFF => Some("GR"),
        // Egypt (010000-017FFF)
        0x010000..=0x017FFF => Some("EG"),
        // Brazil (E40000-E7FFFF)
        0xE40000..=0xE7FFFF => Some("BR"),
        _ => None,
    }
}

/// Return 2-letter country code from an MMSI prefix (first 3 digits).
pub fn mmsi_country(mmsi: &str) -> Option<&'static str> {
    if mmsi.len() < 3 {
        return None;
    }
    match &mmsi[..3] {
        // United States
        "338" | "366" | "367" | "368" | "369" => Some("US"),
        // Russia
        "273" => Some("RU"),
        // Iran
        "422" => Some("IR"),
        // Israel
        "428" => Some("IL"),
        // France
        "226" | "227" | "228" => Some("FR"),
        // United Kingdom
        "230" | "231" | "232" | "233" | "234" | "235" => Some("GB"),
        // Germany
        "211" | "218" => Some("DE"),
        // Netherlands
        "244" => Some("NL"),
        // Norway
        "246" => Some("NO"),
        // Ukraine
        "351" => Some("UA"),
        // China
        "412" | "413" | "414" => Some("CN"),
        // Japan
        "431" | "432" => Some("JP"),
        // South Korea
        "440" | "441" => Some("KR"),
        // Indonesia
        "525" => Some("ID"),
        // Australia
        "503" => Some("AU"),
        // Liberia (flag of convenience)
        "636" => Some("LR"),
        // Marshall Islands (flag of convenience)
        "538" => Some("MH"),
        // Panama (flag of convenience)
        "370" | "371" | "372" | "373" | "374" | "375" => Some("PA"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── region_for_country ──────────────────────────────────────────────

    #[test]
    fn region_for_country_eastern_europe() {
        assert_eq!(region_for_country("UA"), Some("eastern-europe"));
        assert_eq!(region_for_country("RU"), Some("eastern-europe"));
    }

    #[test]
    fn region_for_country_middle_east() {
        assert_eq!(region_for_country("IL"), Some("middle-east"));
        assert_eq!(region_for_country("PS"), Some("middle-east"));
        assert_eq!(region_for_country("SY"), Some("middle-east"));
        assert_eq!(region_for_country("IR"), Some("middle-east"));
        assert_eq!(region_for_country("LB"), Some("middle-east"));
        assert_eq!(region_for_country("IQ"), Some("middle-east"));
        assert_eq!(region_for_country("YE"), Some("middle-east"));
        assert_eq!(region_for_country("TR"), Some("middle-east"));
        assert_eq!(region_for_country("CY"), Some("middle-east"));
    }

    #[test]
    fn region_for_country_africa() {
        assert_eq!(region_for_country("EG"), Some("africa"));
        assert_eq!(region_for_country("SD"), Some("africa"));
        assert_eq!(region_for_country("SO"), Some("africa"));
        assert_eq!(region_for_country("LY"), Some("africa"));
    }

    #[test]
    fn region_for_country_southeast_asia() {
        assert_eq!(region_for_country("MM"), Some("southeast-asia"));
        assert_eq!(region_for_country("TH"), Some("southeast-asia"));
        assert_eq!(region_for_country("PH"), Some("southeast-asia"));
        assert_eq!(region_for_country("ID"), Some("southeast-asia"));
    }

    #[test]
    fn region_for_country_east_asia() {
        assert_eq!(region_for_country("TW"), Some("east-asia"));
        assert_eq!(region_for_country("CN"), Some("east-asia"));
        assert_eq!(region_for_country("JP"), Some("east-asia"));
        assert_eq!(region_for_country("KR"), Some("east-asia"));
        assert_eq!(region_for_country("KP"), Some("east-asia"));
    }

    #[test]
    fn region_for_country_case_insensitive() {
        assert_eq!(region_for_country("ua"), Some("eastern-europe"));
        assert_eq!(region_for_country("Ua"), Some("eastern-europe"));
        assert_eq!(region_for_country("il"), Some("middle-east"));
        assert_eq!(region_for_country("tw"), Some("east-asia"));
        assert_eq!(region_for_country("mm"), Some("southeast-asia"));
    }

    #[test]
    fn region_for_country_unknown() {
        assert_eq!(region_for_country("XX"), None);
        assert_eq!(region_for_country("US"), None);
        assert_eq!(region_for_country("DE"), None);
        assert_eq!(region_for_country(""), None);
    }

    // ── region_for_country_name ─────────────────────────────────────────

    #[test]
    fn region_for_country_name_eastern_europe() {
        assert_eq!(region_for_country_name("Ukraine"), Some("eastern-europe"));
        assert_eq!(region_for_country_name("Russia"), Some("eastern-europe"));
    }

    #[test]
    fn region_for_country_name_middle_east() {
        assert_eq!(region_for_country_name("Iran"), Some("middle-east"));
        assert_eq!(region_for_country_name("Israel"), Some("middle-east"));
        assert_eq!(region_for_country_name("Syria"), Some("middle-east"));
        assert_eq!(region_for_country_name("Lebanon"), Some("middle-east"));
        assert_eq!(region_for_country_name("Yemen"), Some("middle-east"));
        assert_eq!(region_for_country_name("Saudi Arabia"), Some("middle-east"));
        assert_eq!(
            region_for_country_name("United Arab Emirates"),
            Some("middle-east")
        );
        assert_eq!(region_for_country_name("Turkey"), Some("middle-east"));
    }

    #[test]
    fn region_for_country_name_africa() {
        assert_eq!(region_for_country_name("Sudan"), Some("africa"));
        assert_eq!(region_for_country_name("Somalia"), Some("africa"));
        assert_eq!(region_for_country_name("Egypt"), Some("africa"));
        assert_eq!(region_for_country_name("Libya"), Some("africa"));
        assert_eq!(region_for_country_name("Djibouti"), Some("africa"));
    }

    #[test]
    fn region_for_country_name_southeast_asia() {
        assert_eq!(region_for_country_name("Myanmar"), Some("southeast-asia"));
        assert_eq!(region_for_country_name("Thailand"), Some("southeast-asia"));
        assert_eq!(
            region_for_country_name("Philippines"),
            Some("southeast-asia")
        );
    }

    #[test]
    fn region_for_country_name_east_asia() {
        assert_eq!(region_for_country_name("Taiwan"), Some("east-asia"));
        assert_eq!(region_for_country_name("China"), Some("east-asia"));
        assert_eq!(region_for_country_name("Japan"), Some("east-asia"));
        assert_eq!(region_for_country_name("South Korea"), Some("east-asia"));
        assert_eq!(region_for_country_name("North Korea"), Some("east-asia"));
    }

    #[test]
    fn region_for_country_name_unknown() {
        assert_eq!(region_for_country_name("Atlantis"), None);
        assert_eq!(region_for_country_name(""), None);
        // Note: this function is case-sensitive (no to_lowercase)
        assert_eq!(region_for_country_name("ukraine"), None);
    }

    // ── region_from_coords ────────────────────────────────────────────

    #[test]
    fn region_from_coords_middle_east() {
        assert_eq!(region_from_coords(35.7, 51.4), Some("middle-east")); // Tehran
        assert_eq!(region_from_coords(32.0, 34.9), Some("middle-east")); // Tel Aviv
        assert_eq!(region_from_coords(33.3, 44.4), Some("middle-east")); // Baghdad
    }

    #[test]
    fn region_from_coords_eastern_europe() {
        assert_eq!(region_from_coords(50.4, 30.5), Some("eastern-europe")); // Kyiv
        assert_eq!(region_from_coords(55.8, 37.6), Some("eastern-europe")); // Moscow
    }

    #[test]
    fn region_from_coords_western_europe() {
        assert_eq!(region_from_coords(51.5, -0.1), Some("western-europe")); // London
        assert_eq!(region_from_coords(48.9, 2.3), Some("western-europe")); // Paris
    }

    #[test]
    fn region_from_coords_east_asia() {
        assert_eq!(region_from_coords(35.7, 139.7), Some("east-asia")); // Tokyo
        assert_eq!(region_from_coords(39.9, 116.4), Some("east-asia")); // Beijing
    }

    // ── region_center ───────────────────────────────────────────────────

    #[test]
    fn region_center_known_regions() {
        assert_eq!(region_center("middle-east"), Some((27.0, 44.0)));
        assert_eq!(region_center("eastern-europe"), Some((48.5, 31.0)));
        assert_eq!(region_center("southeast-asia"), Some((15.0, 105.0)));
        assert_eq!(region_center("east-asia"), Some((35.0, 120.0)));
        assert_eq!(region_center("north-america"), Some((40.0, -100.0)));
        assert_eq!(region_center("south-america"), Some((-15.0, -55.0)));
        assert_eq!(region_center("arctic"), Some((75.0, 0.0)));
    }

    #[test]
    fn region_center_aliases() {
        // "africa" and "sub-saharan-africa" should return the same value
        assert_eq!(region_center("africa"), region_center("sub-saharan-africa"));
        // "central-america" and "caribbean" should return the same value
        assert_eq!(
            region_center("central-america"),
            region_center("caribbean")
        );
    }

    #[test]
    fn region_center_case_insensitive() {
        assert_eq!(region_center("Middle-East"), Some((27.0, 44.0)));
        assert_eq!(region_center("EASTERN-EUROPE"), Some((48.5, 31.0)));
        assert_eq!(region_center("East-Asia"), Some((35.0, 120.0)));
    }

    #[test]
    fn region_center_unknown() {
        assert_eq!(region_center("narnia"), None);
        assert_eq!(region_center(""), None);
    }

    #[test]
    fn region_center_valid_coordinates() {
        let regions = [
            "middle-east",
            "eastern-europe",
            "western-europe",
            "africa",
            "southeast-asia",
            "east-asia",
            "south-asia",
            "central-asia",
            "north-america",
            "south-america",
            "oceania",
            "arctic",
        ];
        for region in &regions {
            let (lat, lon) = region_center(region)
                .unwrap_or_else(|| panic!("region_center({}) should return Some", region));
            assert!(
                (-90.0..=90.0).contains(&lat),
                "Latitude {} for region {} out of range",
                lat,
                region
            );
            assert!(
                (-180.0..=180.0).contains(&lon),
                "Longitude {} for region {} out of range",
                lon,
                region
            );
        }
    }

    // ── country_center ──────────────────────────────────────────────────

    #[test]
    fn country_center_known() {
        assert_eq!(country_center("US"), Some((39.0, -98.0)));
        assert_eq!(country_center("UA"), Some((48.5, 31.2)));
        assert_eq!(country_center("IL"), Some((31.5, 34.8)));
        assert_eq!(country_center("CN"), Some((35.0, 105.0)));
        assert_eq!(country_center("JP"), Some((36.2, 138.3)));
    }

    #[test]
    fn country_center_uk_alias() {
        // Both "GB" and "UK" should work
        assert_eq!(country_center("GB"), Some((54.0, -2.0)));
        assert_eq!(country_center("UK"), Some((54.0, -2.0)));
    }

    #[test]
    fn country_center_case_insensitive() {
        assert_eq!(country_center("us"), Some((39.0, -98.0)));
        assert_eq!(country_center("ua"), Some((48.5, 31.2)));
        assert_eq!(country_center("gb"), Some((54.0, -2.0)));
    }

    #[test]
    fn country_center_unknown() {
        assert_eq!(country_center("XX"), None);
        assert_eq!(country_center(""), None);
    }

    #[test]
    fn country_center_valid_coordinates() {
        let codes = [
            "US", "UA", "RU", "IL", "PS", "SY", "IR", "IQ", "CN", "TW", "JP", "EG", "DE", "FR",
            "IN", "PK", "AF", "MM", "PH",
        ];
        for cc in &codes {
            let (lat, lon) = country_center(cc)
                .unwrap_or_else(|| panic!("country_center({}) should return Some", cc));
            assert!(
                (-90.0..=90.0).contains(&lat),
                "Latitude {} for country {} out of range",
                lat,
                cc
            );
            assert!(
                (-180.0..=180.0).contains(&lon),
                "Longitude {} for country {} out of range",
                lon,
                cc
            );
        }
    }

    // ── geocode_entity ─────────────────────────────────────────────────

    #[test]
    fn geocode_entity_middle_east_cities() {
        assert!(geocode_entity("Baghdad").is_some());
        assert!(geocode_entity("Tehran").is_some());
        assert!(geocode_entity("Damascus").is_some());
        assert!(geocode_entity("Beirut").is_some());
        assert!(geocode_entity("Sanaa").is_some());
        assert!(geocode_entity("Aden").is_some());
        assert!(geocode_entity("Riyadh").is_some());
        assert!(geocode_entity("Jerusalem").is_some());
        assert!(geocode_entity("Tel Aviv").is_some());
        assert!(geocode_entity("Gaza").is_some());
        assert!(geocode_entity("Rafah").is_some());
    }

    #[test]
    fn geocode_entity_eastern_europe_cities() {
        assert!(geocode_entity("Kyiv").is_some());
        assert!(geocode_entity("Kiev").is_some()); // alternate spelling
        assert!(geocode_entity("Moscow").is_some());
        assert!(geocode_entity("Odesa").is_some());
        assert!(geocode_entity("Kharkiv").is_some());
        assert!(geocode_entity("Donetsk").is_some());
        assert!(geocode_entity("Bakhmut").is_some());
        assert!(geocode_entity("Crimea").is_some());
    }

    #[test]
    fn geocode_entity_africa_cities() {
        assert!(geocode_entity("Khartoum").is_some());
        assert!(geocode_entity("Mogadishu").is_some());
        assert!(geocode_entity("Addis Ababa").is_some());
        assert!(geocode_entity("Cairo").is_some());
        assert!(geocode_entity("Nairobi").is_some());
    }

    #[test]
    fn geocode_entity_asia_cities() {
        assert!(geocode_entity("Taipei").is_some());
        assert!(geocode_entity("Seoul").is_some());
        assert!(geocode_entity("Pyongyang").is_some());
        assert!(geocode_entity("Beijing").is_some());
        assert!(geocode_entity("Tokyo").is_some());
        assert!(geocode_entity("Kabul").is_some());
    }

    #[test]
    fn geocode_entity_suffix_stripping() {
        // "Babil Governorate" -> strips suffix -> matches "babil"
        assert!(geocode_entity("Babil Governorate").is_some());
        assert!(geocode_entity("Nineveh Province").is_some());
        assert!(geocode_entity("Donetsk Oblast").is_some());
        assert!(geocode_entity("Darfur Region").is_some());
    }

    #[test]
    fn geocode_entity_case_insensitive() {
        let (lat1, lon1) = geocode_entity("BAGHDAD").unwrap();
        let (lat2, lon2) = geocode_entity("baghdad").unwrap();
        assert!((lat1 - lat2).abs() < 0.01);
        assert!((lon1 - lon2).abs() < 0.01);
    }

    #[test]
    fn geocode_entity_country_fallback() {
        // Country names should fall through to country_center_for_name
        assert!(geocode_entity("Iraq").is_some());
        assert!(geocode_entity("Ukraine").is_some());
        assert!(geocode_entity("Nigeria").is_some());
    }

    #[test]
    fn geocode_entity_strategic_waterways() {
        assert!(geocode_entity("Strait of Hormuz").is_some());
        assert!(geocode_entity("Suez Canal").is_some());
        assert!(geocode_entity("Bab el-Mandeb").is_some());
        assert!(geocode_entity("Taiwan Strait").is_some());
        assert!(geocode_entity("Red Sea").is_some());
    }

    #[test]
    fn geocode_entity_unknown() {
        assert!(geocode_entity("Atlantis").is_none());
        assert!(geocode_entity("").is_none());
        assert!(geocode_entity("Random Unknown Place XYZ").is_none());
    }

    #[test]
    fn geocode_entity_valid_coordinates() {
        // Ensure all returned coordinates are in valid ranges
        let test_names = [
            "Baghdad", "Tehran", "Damascus", "Kyiv", "Moscow", "Khartoum",
            "Seoul", "Taipei", "Gaza", "Aden", "Bakhmut", "Natanz",
            "Bab el-Mandeb", "Suez Canal", "London", "Washington",
        ];
        for name in &test_names {
            if let Some((lat, lon)) = geocode_entity(name) {
                assert!(
                    (-90.0..=90.0).contains(&lat),
                    "Latitude {} for {} out of range", lat, name
                );
                assert!(
                    (-180.0..=180.0).contains(&lon),
                    "Longitude {} for {} out of range", lon, name
                );
            }
        }
    }

    #[test]
    fn geocode_entity_iranian_nuclear_sites() {
        // These are critical for correlating Telegram OSINT about Iran
        assert!(geocode_entity("Natanz").is_some());
        assert!(geocode_entity("Fordow").is_some());
        assert!(geocode_entity("Bushehr").is_some());
        assert!(geocode_entity("Bandar Abbas").is_some());
        assert!(geocode_entity("Isfahan").is_some());
    }

    // ── json_as_f64 ─────────────────────────────────────────────────────

    #[test]
    fn json_as_f64_number() {
        assert_eq!(json_as_f64(&json!(42.5)), Some(42.5));
        assert_eq!(json_as_f64(&json!(0)), Some(0.0));
        assert_eq!(json_as_f64(&json!(-17)), Some(-17.0));
        assert_eq!(json_as_f64(&json!(1e10)), Some(1e10));
    }

    #[test]
    fn json_as_f64_integer() {
        assert_eq!(json_as_f64(&json!(100)), Some(100.0));
    }

    #[test]
    fn json_as_f64_string_numeric() {
        assert_eq!(json_as_f64(&json!("123.45")), Some(123.45));
        assert_eq!(json_as_f64(&json!("0")), Some(0.0));
        assert_eq!(json_as_f64(&json!("-9.8")), Some(-9.8));
        assert_eq!(json_as_f64(&json!("1e5")), Some(1e5));
    }

    #[test]
    fn json_as_f64_null() {
        assert_eq!(json_as_f64(&json!(null)), None);
    }

    #[test]
    fn json_as_f64_non_numeric_string() {
        assert_eq!(json_as_f64(&json!("hello")), None);
        assert_eq!(json_as_f64(&json!("")), None);
        assert_eq!(json_as_f64(&json!("12.34.56")), None);
    }

    #[test]
    fn json_as_f64_bool() {
        // booleans are not numeric
        assert_eq!(json_as_f64(&json!(true)), None);
        assert_eq!(json_as_f64(&json!(false)), None);
    }

    #[test]
    fn json_as_f64_object_and_array() {
        assert_eq!(json_as_f64(&json!({})), None);
        assert_eq!(json_as_f64(&json!([])), None);
        assert_eq!(json_as_f64(&json!([1, 2])), None);
    }

    // ── urlencode ───────────────────────────────────────────────────────

    #[test]
    fn urlencode_unreserved_passthrough() {
        assert_eq!(urlencode("abc"), "abc");
        assert_eq!(urlencode("ABC"), "ABC");
        assert_eq!(urlencode("0123456789"), "0123456789");
        assert_eq!(urlencode("-_.~"), "-_.~");
    }

    #[test]
    fn urlencode_spaces() {
        assert_eq!(urlencode("hello world"), "hello%20world");
    }

    #[test]
    fn urlencode_special_characters() {
        assert_eq!(urlencode("a&b"), "a%26b");
        assert_eq!(urlencode("a=b"), "a%3Db");
        assert_eq!(urlencode("foo?bar"), "foo%3Fbar");
        assert_eq!(urlencode("100%"), "100%25");
        assert_eq!(urlencode("a/b"), "a%2Fb");
        assert_eq!(urlencode("key+value"), "key%2Bvalue");
    }

    #[test]
    fn urlencode_empty_string() {
        assert_eq!(urlencode(""), "");
    }

    #[test]
    fn urlencode_all_reserved() {
        // '!' is not in the unreserved set, should be encoded
        assert_eq!(urlencode("!"), "%21");
        assert_eq!(urlencode("#"), "%23");
        assert_eq!(urlencode("@"), "%40");
    }

    // ── callsign_country ────────────────────────────────────────────────

    #[test]
    fn callsign_country_us_prefixes() {
        assert_eq!(callsign_country("REACH01"), Some("US"));
        assert_eq!(callsign_country("RCH123"), Some("US"));
        assert_eq!(callsign_country("FORTE12"), Some("US"));
        assert_eq!(callsign_country("PAT01"), Some("US"));
        assert_eq!(callsign_country("MOOSE01"), Some("US"));
        assert_eq!(callsign_country("BFLO01"), Some("US"));
        assert_eq!(callsign_country("SENTRY60"), Some("US"));
        assert_eq!(callsign_country("HOMER01"), Some("US"));
        assert_eq!(callsign_country("ETHYL99"), Some("US"));
        assert_eq!(callsign_country("EVAC01"), Some("US"));
        assert_eq!(callsign_country("JAKE01"), Some("US"));
    }

    #[test]
    fn callsign_country_gb() {
        assert_eq!(callsign_country("RAF01"), Some("GB"));
        assert_eq!(callsign_country("ASCOT123"), Some("GB"));
    }

    #[test]
    fn callsign_country_fr() {
        assert_eq!(callsign_country("FAF001"), Some("FR"));
        assert_eq!(callsign_country("RFR01"), Some("FR"));
        assert_eq!(callsign_country("CTM01"), Some("FR"));
    }

    #[test]
    fn callsign_country_de() {
        assert_eq!(callsign_country("GAF001"), Some("DE"));
    }

    #[test]
    fn callsign_country_il() {
        assert_eq!(callsign_country("IAF001"), Some("IL"));
    }

    #[test]
    fn callsign_country_au() {
        assert_eq!(callsign_country("RRR01"), Some("AU"));
    }

    #[test]
    fn callsign_country_other_nations() {
        assert_eq!(callsign_country("IAM01"), Some("IT"));
        assert_eq!(callsign_country("BAF01"), Some("BE"));
        assert_eq!(callsign_country("CFC01"), Some("CA"));
        assert_eq!(callsign_country("SWS01"), Some("SE"));
        assert_eq!(callsign_country("NOR01"), Some("NO"));
        assert_eq!(callsign_country("TUR01"), Some("TR"));
        assert_eq!(callsign_country("THK01"), Some("TR"));
        assert_eq!(callsign_country("KNG01"), Some("SA"));
        assert_eq!(callsign_country("PLF01"), Some("PL"));
        assert_eq!(callsign_country("PAF01"), Some("PL"));
        assert_eq!(callsign_country("HAF01"), Some("GR"));
        assert_eq!(callsign_country("HEL01"), Some("GR"));
        assert_eq!(callsign_country("JAF01"), Some("JO"));
        assert_eq!(callsign_country("EGF01"), Some("EG"));
        assert_eq!(callsign_country("RSD01"), Some("RU"));
        assert_eq!(callsign_country("DUKE01"), Some("NATO"));
        assert_eq!(callsign_country("NATO01"), Some("NATO"));
    }

    #[test]
    fn callsign_country_case_insensitive() {
        assert_eq!(callsign_country("reach01"), Some("US"));
        assert_eq!(callsign_country("Reach01"), Some("US"));
        assert_eq!(callsign_country("raf01"), Some("GB"));
        assert_eq!(callsign_country("gaf001"), Some("DE"));
        assert_eq!(callsign_country("forte10"), Some("US"));
    }

    #[test]
    fn callsign_country_unknown() {
        assert_eq!(callsign_country("UNKNOWN"), None);
        assert_eq!(callsign_country("XYZ123"), None);
        assert_eq!(callsign_country(""), None);
        assert_eq!(callsign_country("AB"), None);
    }

    #[test]
    fn callsign_country_longer_prefix_priority() {
        // SENTRY (6 chars) is checked before shorter prefixes
        assert_eq!(callsign_country("SENTRY01"), Some("US"));
        // REACH (5 chars) is checked before RCH (3 chars) -- both map to US anyway
        assert_eq!(callsign_country("REACH01"), Some("US"));
        // ASCOT (5 chars) is checked before shorter prefixes
        assert_eq!(callsign_country("ASCOT99"), Some("GB"));
    }

    // ── icao_hex_country ────────────────────────────────────────────────

    #[test]
    fn icao_hex_country_us() {
        // Start of US range
        assert_eq!(icao_hex_country("A00000"), Some("US"));
        // End of US range
        assert_eq!(icao_hex_country("ADF7C7"), Some("US"));
        // Mid-range US
        assert_eq!(icao_hex_country("A50000"), Some("US"));
    }

    #[test]
    fn icao_hex_country_us_boundary() {
        // Just past end of US range
        assert_eq!(icao_hex_country("ADF7C8"), None);
        // Just before start of US range
        assert_eq!(icao_hex_country("9FFFFF"), None);
    }

    #[test]
    fn icao_hex_country_gb() {
        assert_eq!(icao_hex_country("400000"), Some("GB"));
        assert_eq!(icao_hex_country("43FFFF"), Some("GB"));
    }

    #[test]
    fn icao_hex_country_ru() {
        assert_eq!(icao_hex_country("140000"), Some("RU"));
        assert_eq!(icao_hex_country("17FFFF"), Some("RU"));
    }

    #[test]
    fn icao_hex_country_au() {
        assert_eq!(icao_hex_country("7C0000"), Some("AU"));
        assert_eq!(icao_hex_country("7FFFFF"), Some("AU"));
    }

    #[test]
    fn icao_hex_country_other_nations() {
        assert_eq!(icao_hex_country("3C0000"), Some("DE"));
        assert_eq!(icao_hex_country("380000"), Some("FR"));
        assert_eq!(icao_hex_country("300000"), Some("IT"));
        assert_eq!(icao_hex_country("340000"), Some("ES"));
        assert_eq!(icao_hex_country("780000"), Some("CN"));
        assert_eq!(icao_hex_country("738000"), Some("IL"));
        assert_eq!(icao_hex_country("730000"), Some("IR"));
        assert_eq!(icao_hex_country("440000"), Some("TR"));
        assert_eq!(icao_hex_country("508000"), Some("UA"));
        assert_eq!(icao_hex_country("C00000"), Some("CA"));
        assert_eq!(icao_hex_country("C80000"), Some("NZ"));
        assert_eq!(icao_hex_country("840000"), Some("JP"));
        assert_eq!(icao_hex_country("710000"), Some("KR"));
        assert_eq!(icao_hex_country("800000"), Some("IN"));
        assert_eq!(icao_hex_country("480000"), Some("NL"));
        assert_eq!(icao_hex_country("448000"), Some("BE"));
        assert_eq!(icao_hex_country("478000"), Some("NO"));
        assert_eq!(icao_hex_country("4A0000"), Some("SE"));
        assert_eq!(icao_hex_country("488000"), Some("PL"));
        assert_eq!(icao_hex_country("468000"), Some("GR"));
        assert_eq!(icao_hex_country("010000"), Some("EG"));
        assert_eq!(icao_hex_country("E40000"), Some("BR"));
    }

    #[test]
    fn icao_hex_country_invalid_hex() {
        assert_eq!(icao_hex_country("ZZZZZZ"), None);
        assert_eq!(icao_hex_country("not_hex"), None);
    }

    #[test]
    fn icao_hex_country_empty_string() {
        assert_eq!(icao_hex_country(""), None);
    }

    #[test]
    fn icao_hex_country_zero() {
        // 000000 falls before any allocated range
        assert_eq!(icao_hex_country("000000"), None);
    }

    #[test]
    fn icao_hex_country_whitespace_trimmed() {
        // The function trims whitespace
        assert_eq!(icao_hex_country(" A00000 "), Some("US"));
        assert_eq!(icao_hex_country("  400000  "), Some("GB"));
    }

    // ── mmsi_country ────────────────────────────────────────────────────

    #[test]
    fn mmsi_country_us() {
        assert_eq!(mmsi_country("366123456"), Some("US"));
        assert_eq!(mmsi_country("367123456"), Some("US"));
        assert_eq!(mmsi_country("368123456"), Some("US"));
        assert_eq!(mmsi_country("369123456"), Some("US"));
        assert_eq!(mmsi_country("338123456"), Some("US"));
    }

    #[test]
    fn mmsi_country_ru() {
        assert_eq!(mmsi_country("273123456"), Some("RU"));
    }

    #[test]
    fn mmsi_country_cn() {
        assert_eq!(mmsi_country("412123456"), Some("CN"));
        assert_eq!(mmsi_country("413123456"), Some("CN"));
        assert_eq!(mmsi_country("414123456"), Some("CN"));
    }

    #[test]
    fn mmsi_country_flags_of_convenience() {
        // Liberia
        assert_eq!(mmsi_country("636123456"), Some("LR"));
        // Panama
        assert_eq!(mmsi_country("370123456"), Some("PA"));
        assert_eq!(mmsi_country("371123456"), Some("PA"));
        assert_eq!(mmsi_country("372123456"), Some("PA"));
        // Marshall Islands
        assert_eq!(mmsi_country("538123456"), Some("MH"));
    }

    #[test]
    fn mmsi_country_other_nations() {
        assert_eq!(mmsi_country("422123456"), Some("IR"));
        assert_eq!(mmsi_country("428123456"), Some("IL"));
        assert_eq!(mmsi_country("226123456"), Some("FR"));
        assert_eq!(mmsi_country("232123456"), Some("GB"));
        assert_eq!(mmsi_country("211123456"), Some("DE"));
        assert_eq!(mmsi_country("244123456"), Some("NL"));
        assert_eq!(mmsi_country("246123456"), Some("NO"));
        assert_eq!(mmsi_country("351123456"), Some("UA"));
        assert_eq!(mmsi_country("431123456"), Some("JP"));
        assert_eq!(mmsi_country("440123456"), Some("KR"));
        assert_eq!(mmsi_country("525123456"), Some("ID"));
        assert_eq!(mmsi_country("503123456"), Some("AU"));
    }

    #[test]
    fn mmsi_country_too_short() {
        assert_eq!(mmsi_country(""), None);
        assert_eq!(mmsi_country("36"), None);
        assert_eq!(mmsi_country("1"), None);
    }

    #[test]
    fn mmsi_country_exactly_three_digits() {
        // 3 chars is the minimum required; prefix-only should still match
        assert_eq!(mmsi_country("366"), Some("US"));
        assert_eq!(mmsi_country("273"), Some("RU"));
    }

    #[test]
    fn mmsi_country_unknown_prefix() {
        assert_eq!(mmsi_country("999123456"), None);
        assert_eq!(mmsi_country("000123456"), None);
        assert_eq!(mmsi_country("123456789"), None);
    }

    // ── country_center_for_name ─────────────────────────────────────────

    #[test]
    fn country_center_for_name_known() {
        assert!(country_center_for_name("Iraq").is_some());
        assert!(country_center_for_name("Syria").is_some());
        assert!(country_center_for_name("Ukraine").is_some());
        assert!(country_center_for_name("iran").is_some());
    }

    #[test]
    fn country_center_for_name_iraq_not_region_centroid() {
        let (lat, _lon) = country_center_for_name("Iraq").unwrap();
        let (region_lat, _) = region_center("middle-east").unwrap();
        assert!((lat - region_lat).abs() > 5.0);
    }

    #[test]
    fn country_center_for_name_unknown() {
        assert_eq!(country_center_for_name("Atlantis"), None);
        assert_eq!(country_center_for_name(""), None);
    }

    // ── geocode_entity ──────────────────────────────────────────────────

    #[test]
    fn geocode_entity_cities() {
        let (lat, lon) = geocode_entity("Baghdad").unwrap();
        assert!((lat - 33.31).abs() < 0.1);
        assert!((lon - 44.37).abs() < 0.1);

        let (lat, lon) = geocode_entity("Kyiv").unwrap();
        assert!((lat - 50.45).abs() < 0.1);
        assert!((lon - 30.52).abs() < 0.1);
    }

    // ── is_region_centroid ──────────────────────────────────────────────

    #[test]
    fn is_region_centroid_matches_known() {
        assert!(is_region_centroid(27.0, 44.0));
        assert!(is_region_centroid(48.5, 31.0));
        assert!(is_region_centroid(35.0, 120.0));
    }

    #[test]
    fn is_region_centroid_near_match() {
        assert!(is_region_centroid(27.02, 44.01));
        assert!(is_region_centroid(48.48, 31.03));
    }

    #[test]
    fn is_region_centroid_rejects_real_coords() {
        assert!(!is_region_centroid(33.3, 44.4));
        assert!(!is_region_centroid(50.45, 30.52));
        assert!(!is_region_centroid(0.0, 0.0));
    }
}
