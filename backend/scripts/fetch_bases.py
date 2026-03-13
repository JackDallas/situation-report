#!/usr/bin/env python3
"""Fetch military bases and government buildings from OpenStreetMap Overpass API."""

import json
import time
import urllib.request
import urllib.parse
import sys

OVERPASS_URL = "https://overpass-api.de/api/interpreter"

# Bounding boxes: [south, west, north, east]
REGIONS = {
    "middle_east": [12.0, 25.0, 42.0, 63.0],
    "eastern_europe": [44.0, 22.0, 56.0, 40.0],
    "east_asia": [20.0, 100.0, 45.0, 145.0],
    "horn_of_africa": [-2.0, 32.0, 18.0, 52.0],
}

QUERY_TEMPLATE = """
[out:json][timeout:120];
(
  node["military"="base"]({bbox});
  way["military"="base"]({bbox});
  node["military"="airfield"]({bbox});
  way["military"="airfield"]({bbox});
  node["military"="naval_base"]({bbox});
  way["military"="naval_base"]({bbox});
  node["aeroway"="aerodrome"]["military"="yes"]({bbox});
  way["aeroway"="aerodrome"]["military"="yes"]({bbox});
);
out center;
"""

def fetch_overpass(query: str) -> dict:
    data = urllib.parse.urlencode({"data": query}).encode()
    req = urllib.request.Request(OVERPASS_URL, data=data)
    req.add_header("User-Agent", "SituationReport/1.0 (military base reference layer)")
    with urllib.request.urlopen(req, timeout=180) as resp:
        return json.loads(resp.read())

def element_to_feature(el: dict, region: str) -> dict | None:
    tags = el.get("tags", {})
    name = tags.get("name", tags.get("name:en", ""))

    # Get coordinates
    if el["type"] == "node":
        lat, lon = el.get("lat"), el.get("lon")
    elif "center" in el:
        lat, lon = el["center"].get("lat"), el["center"].get("lon")
    else:
        return None

    if lat is None or lon is None:
        return None

    mil_type = tags.get("military", "base")
    if mil_type == "yes":
        mil_type = "airfield" if "aeroway" in tags else "base"

    country = tags.get("addr:country", tags.get("country", ""))

    return {
        "type": "Feature",
        "geometry": {
            "type": "Point",
            "coordinates": [round(lon, 5), round(lat, 5)]
        },
        "properties": {
            "name": name,
            "type": mil_type,
            "country": country,
            "region": region,
            "operator": tags.get("operator", ""),
        }
    }

def main():
    all_features = []
    seen = set()

    for region_name, bbox in REGIONS.items():
        bbox_str = f"{bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}"
        query = QUERY_TEMPLATE.replace("{bbox}", bbox_str)

        print(f"Fetching {region_name}...", file=sys.stderr)
        try:
            result = fetch_overpass(query)
        except Exception as e:
            print(f"  Error: {e}", file=sys.stderr)
            continue

        elements = result.get("elements", [])
        print(f"  Got {len(elements)} elements", file=sys.stderr)

        for el in elements:
            feature = element_to_feature(el, region_name)
            if feature is None:
                continue
            # Deduplicate by coordinates
            key = (feature["geometry"]["coordinates"][0], feature["geometry"]["coordinates"][1])
            if key in seen:
                continue
            seen.add(key)
            all_features.append(feature)

        time.sleep(2)  # Be nice to Overpass API

    geojson = {
        "type": "FeatureCollection",
        "features": all_features
    }

    print(f"Total: {len(all_features)} features", file=sys.stderr)
    print(json.dumps(geojson, indent=2))

if __name__ == "__main__":
    main()
