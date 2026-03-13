# Map Performance Optimization Analysis

**Date**: 2026-03-04
**Scope**: MapPanel.svelte, map.svelte.ts, events.svelte.ts, sse.ts, position-interpolator.ts, backend routes

---

## Executive Summary

The map has several significant performance bottlenecks. The most impactful is the
GeoJSON source data being rebuilt on every clock tick (every 5 seconds) via a
`$effect` that deep-clones the entire feature collection, iterates every feature,
and calls `setData()` -- even when nothing has changed. Combined with the
position interpolator running at 10 Hz and setting `mapStore.positions` on every
tick (triggering another `$effect` -> `setData()` cycle), the map is doing far
more GPU/CPU work than necessary.

The good news: the existing architecture already has several smart mitigations
(viewport-bounded position polling, zoom-based position filtering, maxFeatures
cap, age-based opacity). The bottlenecks are mostly in how reactive state changes
propagate to MapLibre source updates, not in the data volumes themselves.

---

## Bottleneck 1: GeoJSON Source Rebuilt Every 5 Seconds (HIGH IMPACT)

### Current behavior

In `MapPanel.svelte` lines 779-842, the `$effect` that updates the `events`
GeoJSON source depends on:

- `clockStore.now` (updates every 5 seconds)
- `mapStore.geoData` (updates on each SSE event)
- `mapStore.recentlyUpdated` (updates on each SSE event + every 500ms prune)
- `mapStore.timeRange` (updates on timeline slider interaction)

Every time ANY of these dependencies change, the effect:

1. **Deep-clones the entire geoData** via `JSON.parse(JSON.stringify(mapStore.geoData))` -- O(n) where n = up to 2000 features
2. **Filters all features** against the time range -- O(n)
3. **Iterates all features** to compute `age_minutes` and `recently_updated` -- O(n)
4. **Builds pulse features** array -- O(k) where k = recently updated count
5. **Calls `map.getSource('events').setData(raw)`** -- forces MapLibre to re-parse, diff, and re-tile the entire source

At 2000 features, this means roughly 6000+ feature property accesses every 5
seconds just from the clock tick, plus a full GeoJSON re-parse by MapLibre.

### Why it matters

The `JSON.parse(JSON.stringify(...))` deep clone is particularly expensive because
the `payload` field on each feature can be a large nested object (enrichment data,
NOTAM decode, etc). For 2000 features with enrichment payloads, this clone alone
can be 2-5 MB of JSON serialization/deserialization.

### Root cause

The `age_minutes` property needs to update for the opacity interpolation paint
expression to work. But age changes slowly -- an event at 119 minutes vs 120
minutes is not visually distinguishable. The clock ticks every 5s but age is
measured in minutes.

---

## Bottleneck 2: Position Interpolator Triggers Full Source Rebuild at 10 Hz (HIGH IMPACT)

### Current behavior

The position interpolator in `position-interpolator.ts` runs via
`requestAnimationFrame` with a 100ms throttle (10 Hz). On every tick it:

1. Creates a new `Map(mapStore.positions)` clone
2. Iterates all base positions
3. Extrapolates new lat/lng for moving entities
4. Sets `mapStore.positions = interpolated` (a new Map reference)

This assignment triggers the `$effect` in `MapPanel.svelte` (lines 902-990) that:

1. Iterates ALL positions
2. Classifies each into priority tiers
3. Builds GeoJSON features array
4. Calls `map.getSource('positions').setData(...)` -- full source rebuild

At 10 Hz with hundreds of positions, this is the single largest source of
unnecessary work. Each `setData()` call forces MapLibre to diff the entire
GeoJSON tree and update WebGL buffers.

### Compounding factors

- The same `$effect` also depends on `mapStore.hideGroundPlanes`, so toggling that
  preference triggers a rebuild too (acceptable, rare).
- The `map.getZoom()` call inside the effect means zoom-based filtering is
  recalculated on every interpolation tick, not just on zoom changes.
- Trail lines have their own `$effect` (lines 993-1022) that depends on
  `mapStore.positionHistory`, which changes whenever positions update. However,
  trails only need updating every 30s (when new positions arrive from polling),
  not at 10 Hz.

---

## Bottleneck 3: No Event Clustering at Low Zoom Levels (MEDIUM IMPACT)

### Current behavior

All up to 2000 event features are sent to MapLibre at every zoom level. At zoom 3
(global view), 2000 overlapping circles create:

- GPU overdraw (circles drawn on top of each other)
- Confusing visual noise (impossible to distinguish individual events)
- Unnecessary hit-testing complexity for click/hover handlers

### What exists

MapLibre has a built-in clustering feature for GeoJSON sources. The `events`
source is created at line 240 without `cluster: true`. The heatmap layer partially
addresses this at low zoom (maxzoom 8) for conflict/thermal/nuclear/GPS events,
but the circle markers are still drawn underneath.

### What is missing

No server-side or client-side clustering. No zoom-dependent feature reduction for
events (positions DO have zoom-based filtering at lines 927-928, but events do
not).

---

## Bottleneck 4: Events Not Filtered by Viewport (MEDIUM IMPACT)

### Current behavior

On initial load, `loadInitialData()` in `sse.ts` fetches up to 1000 geo events
from the last 12 hours via `api.getEventsGeo()`. This queries the DB with a time
filter but NO spatial filter:

```sql
SELECT ... FROM events
WHERE location IS NOT NULL
  AND ($1::timestamptz IS NULL OR event_time >= $1)
  AND ($3::text[] IS NULL OR event_type = ANY($3))
ORDER BY event_time DESC
LIMIT $2
```

So a user zoomed into Syria still downloads all 1000 events globally, including
events in East Asia, South America, etc. that are completely off-screen.

Positions ARE viewport-filtered (bbox parameters sent in `pollPositions()`), but
events are not.

### After initial load

SSE events arrive one-by-one and are appended to `mapStore.geoData` via
`addEventFeature()`. These are never pruned by viewport -- they accumulate
globally up to `maxFeatures = 2000`. This is actually correct for SSE (you want
to keep events for when the user pans), but the initial load could be smarter.

---

## Bottleneck 5: Pulse Animation Overhead (LOW-MEDIUM IMPACT)

### Current behavior

The pulse animation system has THREE overlapping mechanisms:

1. **`pruneRecentlyUpdated()` runs every 500ms** via `setInterval` (MapPanel line
   773). It creates a new `Map(this.recentlyUpdated)` on every call, iterates all
   entries, and sets `this.recentlyUpdated = next` if anything changed. This
   triggers the main event `$effect` because `recentlyUpdated` is a dependency.

2. **`recently_updated` property** is stamped on every feature in the main event
   `$effect`, requiring iteration of the full `recentlyUpdated` map per feature.

3. **Pulse ring features** are built as a separate GeoJSON source (`pulse`) with
   its own `setData()` call on every render cycle.

The 500ms prune interval means the main event `$effect` fires at LEAST every 500ms
(not just every 5s from the clock), because `pruneRecentlyUpdated()` always creates
a new Map reference even when nothing changed (when there are active pulses).

### The actual pulse cycle

When a new event arrives:
1. `addEventFeature()` sets `recentlyUpdated` -> triggers event `$effect`
2. Over the next 5 seconds, `pruneRecentlyUpdated()` fires 10 times at 500ms intervals
3. Each prune that changes the map triggers the event `$effect` again
4. Each event `$effect` does the full deep-clone + iterate + setData cycle

So a single new event causes roughly 10 full GeoJSON rebuilds over 5 seconds.

---

## Bottleneck 6: Full Object Spread on Each SSE Event (LOW IMPACT)

### Current behavior

`mapStore.addEventFeature()` (lines 260-291) creates a new `geoData` object on
every SSE event:

```typescript
const features = [feature, ...this.geoData.features].slice(0, this.maxFeatures);
this.geoData = { type: 'FeatureCollection', features };
```

This copies the entire features array (up to 2000 items) on every event. Similarly,
`updateGeoData()` iterates all features to build `nextSourceIdSet` and check for
new source_ids.

At typical SSE rates (5-20 events/minute for pass-through types), this is not a
major bottleneck, but it compounds with the `$effect` that fires on each `geoData`
change.

---

## Bottleneck 7: Trail Lines Rebuilt at Position Interpolation Rate (LOW IMPACT)

### Current behavior

The trail `$effect` (lines 993-1022) depends on `mapStore.positionHistory`. The
position interpolator does NOT modify `positionHistory`, so this effect only fires
when new positions arrive from polling (every 30s) or when a trail is loaded via
`loadEntityTrail()`. This is actually fine. However, the effect does depend on
`mapStore.positions` (via the `mapStore.positions.get(entityId)` call at line 998),
which means it DOES fire at 10 Hz from the interpolator.

---

## Recommended Optimizations (Ranked by Impact)

### 1. Debounce/Batch the Events Source Update (HIGH IMPACT, EASY WIN)

Replace the reactive `$effect` with a manual update loop that batches changes and
only calls `setData()` when something actually changed, at most every 1-2 seconds.

```typescript
// In MapPanel.svelte, replace the $effect with a batched updater
let eventUpdateTimer: ReturnType<typeof setInterval> | null = null;
let lastEventDataVersion = 0;

function updateEventSource() {
    if (!mapLoaded || !map?.getSource('events')) return;

    const now = Date.now();
    const raw = mapStore.geoData; // DON'T deep clone
    const startMs = mapStore.timeRange.start.getTime();
    const endMs = mapStore.isLive ? now : mapStore.timeRange.end.getTime();
    const recentlyUpdated = mapStore.recentlyUpdated;

    // Build features with age and pulse info
    const features: any[] = [];
    const pulseFeatures: any[] = [];

    for (const f of raw.features) {
        // Time range filter
        if (f.properties?.event_time && f.properties.event_type !== 'geo_event') {
            try {
                const t = new Date(f.properties.event_time).getTime();
                if (t < startMs || t > endMs) continue;
            } catch { /* pass */ }
        }

        // Compute age_minutes inline (don't modify original)
        let ageMinutes = 0;
        if (f.properties?.event_time) {
            try {
                ageMinutes = Math.max(0, Math.floor(
                    (now - new Date(f.properties.event_time).getTime()) / 60000
                ));
            } catch { /* 0 */ }
        }

        const sid = f.properties?.source_id;
        const pulseTs = sid ? recentlyUpdated.get(sid) : undefined;
        const isRecent = pulseTs != null && (now - pulseTs <= 5000);

        // Create a lightweight wrapper instead of deep clone
        features.push({
            type: 'Feature',
            geometry: f.geometry,
            properties: {
                ...f.properties,
                age_minutes: ageMinutes,
                recently_updated: isRecent,
                // Omit payload from map properties to reduce serialization
                payload: undefined
            }
        });

        if (isRecent && f.geometry?.coordinates) {
            const elapsed = now - pulseTs;
            const progress = Math.min(elapsed / 5000, 1);
            pulseFeatures.push({
                type: 'Feature',
                geometry: f.geometry,
                properties: {
                    pulse_radius: 8 + progress * 20,
                    pulse_opacity: Math.max(0, 1 - progress)
                }
            });
        }
    }

    map.getSource('events').setData({
        type: 'FeatureCollection',
        features
    });

    if (map.getSource('pulse')) {
        map.getSource('pulse').setData({
            type: 'FeatureCollection',
            features: pulseFeatures
        });
    }
}

onMount(() => {
    // ... existing map init ...

    // Replace $effect with interval-based update
    eventUpdateTimer = setInterval(updateEventSource, 2000);
});

onDestroy(() => {
    if (eventUpdateTimer) clearInterval(eventUpdateTimer);
});
```

**Key improvement**: No `JSON.parse(JSON.stringify())` deep clone. Instead, create
lightweight feature wrappers that share the original geometry object reference.
Also, strip the `payload` from map feature properties since MapLibre never needs
it for rendering (it is only used in popup HTML, which can read from the original
store). This alone could reduce serialization cost by 80%+.

**Difficulty**: Easy. Localized to MapPanel.svelte. ~30 minutes.

---

### 2. Decouple Position Interpolation from GeoJSON Source Updates (HIGH IMPACT, MODERATE)

The interpolator should update MapLibre directly instead of going through the
reactive store.

**Option A: Use MapLibre's `setData()` directly from the interpolator**

```typescript
// In position-interpolator.ts
let mapInstance: any = null;

export function setMapInstance(map: any) {
    mapInstance = map;
}

function tick() {
    // ... existing throttle/checks ...

    // Instead of setting mapStore.positions, update MapLibre directly
    if (!mapInstance?.getSource('positions')) {
        rafId = requestAnimationFrame(tick);
        return;
    }

    const features: any[] = [];
    for (const [entityId, base] of basePositions) {
        if (base.heading == null || base.speed == null || base.speed < 1) {
            // Non-moving: use current position
            const current = basePositions.get(entityId);
            if (current) features.push(buildPositionFeature(current));
            continue;
        }

        const [newLng, newLat] = extrapolate(
            base.latitude, base.longitude,
            base.heading, base.speed, dtSeconds
        );

        features.push(buildPositionFeature({
            ...base,
            latitude: newLat,
            longitude: newLng
        }));
    }

    mapInstance.getSource('positions').setData({
        type: 'FeatureCollection',
        features
    });

    rafId = requestAnimationFrame(tick);
}
```

**Option B: Use MapLibre `setFeatureState` for position updates** (requires feature IDs)

This would require assigning stable IDs to position features and using MapLibre's
feature-state system, which avoids full source rebuilds. This is more complex but
the most performant option.

**Key improvement**: Eliminates the reactive chain: interpolator -> store mutation
-> $effect -> setData. The interpolator updates MapLibre directly. The store is
only updated on actual poll responses (every 30s).

**Difficulty**: Moderate. Requires refactoring position-interpolator.ts and the
position $effect in MapPanel.svelte. ~1-2 hours.

---

### 3. Enable MapLibre Clustering for Events (MEDIUM IMPACT, EASY WIN)

Add clustering to the events GeoJSON source for better low-zoom performance.

```typescript
map.addSource('events', {
    type: 'geojson',
    data: { type: 'FeatureCollection', features: [] },
    cluster: true,
    clusterMaxZoom: 10,   // disable clustering above zoom 10
    clusterRadius: 50,    // pixel radius for clustering
    clusterProperties: {
        // Aggregate max severity for cluster styling
        maxSeverityRank: ['max', [
            'match', ['get', 'severity'],
            'critical', 4,
            'high', 3,
            'medium', 2,
            1
        ]]
    }
});

// Add cluster circle layer
map.addLayer({
    id: 'event-clusters',
    type: 'circle',
    source: 'events',
    filter: ['has', 'point_count'],
    paint: {
        'circle-radius': [
            'step', ['get', 'point_count'],
            15,    // radius for count < 10
            10, 20,  // radius for count 10-50
            50, 25,  // radius for count 50+
            100, 30
        ],
        'circle-color': [
            'step', ['get', 'maxSeverityRank'],
            '#3b82f6',  // low (default)
            2, '#eab308',  // medium
            3, '#f97316',  // high
            4, '#ef4444'   // critical
        ],
        'circle-opacity': 0.7,
        'circle-stroke-width': 2,
        'circle-stroke-color': 'rgba(255,255,255,0.3)'
    }
});

// Cluster count label
map.addLayer({
    id: 'event-cluster-count',
    type: 'symbol',
    source: 'events',
    filter: ['has', 'point_count'],
    layout: {
        'text-field': '{point_count_abbreviated}',
        'text-size': 11,
        'text-font': ['Open Sans Bold']
    },
    paint: {
        'text-color': '#ffffff'
    }
});

// Unclustered points (existing events-circle layer)
// Add filter: ['!', ['has', 'point_count']]
```

**Considerations**:
- The heatmap layer also uses the `events` source. If clustering is enabled,
  heatmap behavior may change (it would use cluster centroids). You may need a
  separate unclustered source for the heatmap.
- The `impact-sites` and `incidents-glow` layers use the same source. They would
  need filter adjustments to exclude cluster points.
- Click-to-zoom on clusters is standard UX and easy to add.

**Difficulty**: Easy for basic clustering. Moderate to handle heatmap + impact
sites interaction. ~1-2 hours.

---

### 4. Strip Payload from Map Feature Properties (EASY WIN, MEDIUM IMPACT)

The `payload` field is included in every GeoJSON feature's properties, but MapLibre
only needs it for popup HTML (which is built on-click). The payload can contain
large enrichment objects, NOTAM data, etc.

```typescript
// In mapStore.addEventFeature(), strip payload before adding to geoData
const feature: GeoJSONFeature = {
    type: 'Feature',
    geometry: { type: 'Point', coordinates: [event.longitude, event.latitude] },
    properties: {
        source_type: event.source_type,
        source_id: event.source_id,
        event_type: event.event_type,
        event_time: event.event_time,
        entity_id: event.entity_id,
        entity_name: event.entity_name,
        severity: event.severity,
        confidence: event.confidence,
        title: event.title,
        region_code: event.region_code
        // payload: OMITTED -- look up from eventStore on click
    }
};
```

For popups, look up the full event from `eventStore.events` by `source_id` (which
is already done as a fallback in `__srOpenDetail`).

Similarly in `get_events_geojson()` on the backend, the payload is serialized into
every GeoJSON feature. A `slim` query parameter could omit it:

```rust
// In get_events_geojson, omit payload for map rendering
"properties": {
    "source_type": e.source_type,
    "source_id": e.source_id,
    "event_type": e.event_type,
    // ... other rendering properties ...
    // "payload": e.payload  // OMITTED for map performance
}
```

**Impact**: Reduces the JSON size of geoData by 60-80%. The deep clone in the
`$effect` processes much less data. MapLibre's internal tile worker parses less
JSON. Network transfer for initial geo load is smaller.

**Difficulty**: Easy. ~30 minutes for frontend, ~15 minutes for backend.

---

### 5. Reduce Pulse Prune Frequency (EASY WIN, LOW-MEDIUM IMPACT)

Change `pruneRecentlyUpdated()` interval from 500ms to 2000ms or 3000ms. The
visual difference is negligible (pulse animations are 5 seconds long).

```typescript
// In MapPanel.svelte onMount
pulseInterval = setInterval(() => {
    mapStore.pruneRecentlyUpdated();
}, 2000); // was 500
```

Also, fix `pruneRecentlyUpdated()` to avoid creating a new Map when nothing changed:

```typescript
pruneRecentlyUpdated(): void {
    if (this.recentlyUpdated.size === 0) return; // fast path
    const now = Date.now();
    let changed = false;
    // Check first, then create new Map only if needed
    for (const [, ts] of this.recentlyUpdated) {
        if (now - ts > MapStore.PULSE_DURATION_MS) {
            changed = true;
            break;
        }
    }
    if (!changed) return;
    const next = new Map<string, number>();
    for (const [sid, ts] of this.recentlyUpdated) {
        if (now - ts <= MapStore.PULSE_DURATION_MS) {
            next.set(sid, ts);
        }
    }
    this.recentlyUpdated = next;
}
```

**Difficulty**: Trivial. 5 minutes.

---

### 6. Add Viewport-Based Event Filtering on Initial Load (MODERATE IMPACT, MODERATE)

Add bbox parameters to the `/api/events/geo` endpoint and use them on initial load.

**Backend change** (`queries.rs`):

```rust
pub async fn get_events_geojson(
    pool: &PgPool,
    since: Option<DateTime<Utc>>,
    limit: i64,
    include_types: Option<&[String]>,
    exclude_types: Option<&[String]>,
    bbox: Option<(f64, f64, f64, f64)>, // NEW
) -> anyhow::Result<serde_json::Value> {
    // Add ST_Within filter similar to query_latest_positions
}
```

**Frontend change** (`sse.ts`):

```typescript
async function loadInitialData() {
    // Wait for map to report initial viewport, then load with bbox
    const bounds = mapStore.viewportBounds;
    // Pass bounds to getEventsGeo if available
}
```

**Consideration**: The user might pan after initial load and expect events in the
new area. The SSE stream only delivers NEW events, not historical ones from a
newly-visible area. Options:
- Load a generous initial bbox (padded 50% beyond viewport)
- Re-fetch geo events on significant viewport changes (but throttled)
- Accept that only new events appear in panned-to areas (current behavior)

**Difficulty**: Moderate. Requires backend query changes + frontend coordination.
~1-2 hours.

---

### 7. Separate Trail Line Dependencies from Position Updates (EASY WIN)

The trail `$effect` reads `mapStore.positions.get(entityId)` to determine
`isMil`/`isFlight` styling. This creates a dependency on `mapStore.positions`,
which changes at 10 Hz from the interpolator. Fix by caching entity type metadata
separately.

```typescript
// In MapPanel.svelte trail $effect, use a separate metadata lookup
$effect(() => {
    if (!mapLoaded || !map?.getSource('trails')) return;
    const history = mapStore.positionHistory; // only dependency we need
    // Cache pos_type per entity to avoid depending on positions
    const features: any[] = [];
    for (const [entityId, trail] of history) {
        if (trail.length < 2) continue;
        // Store entity metadata (military/flight/vessel) in positionHistory
        // or use a separate lookup that doesn't change at 10Hz
        features.push({...});
    }
    map.getSource('trails').setData({...});
});
```

**Difficulty**: Easy. ~15 minutes.

---

### 8. Use `updateData` Instead of `setData` (MapLibre Feature, LOW IMPACT)

MapLibre GL JS v4+ supports `source.updateData()` for incremental GeoJSON updates
(add/remove individual features without rebuilding the entire source). Check which
version is in use:

```typescript
// Instead of rebuilding entire FeatureCollection:
map.getSource('events').setData(fullCollection);

// Use incremental updates:
map.getSource('events').updateData({
    add: [newFeature],
    remove: ['feature-id-to-remove']
});
```

This requires features to have stable `id` fields. Currently features do not have
IDs set.

**Difficulty**: Moderate. Requires feature ID management and version check. ~2 hours.

---

## Summary Table

| # | Optimization | Impact | Difficulty | Est. Time |
|---|---|---|---|---|
| 1 | Debounce/batch event source updates | HIGH | Easy | 30 min |
| 2 | Decouple position interpolation from store | HIGH | Moderate | 1-2 hrs |
| 3 | Enable MapLibre clustering for events | MEDIUM | Easy-Moderate | 1-2 hrs |
| 4 | Strip payload from map feature properties | MEDIUM | Easy | 45 min |
| 5 | Reduce pulse prune frequency + fix no-op prune | LOW-MEDIUM | Trivial | 5 min |
| 6 | Viewport-based event filtering on initial load | MODERATE | Moderate | 1-2 hrs |
| 7 | Separate trail dependencies from position updates | LOW | Easy | 15 min |
| 8 | Use incremental `updateData` API | LOW | Moderate | 2 hrs |

## Recommended Implementation Order

1. **#5** -- Trivial fix, immediate win, zero risk
2. **#4** -- Strip payload from GeoJSON properties, big serialization win
3. **#1** -- Debounce event source updates, eliminates clock-driven churn
4. **#7** -- Fix trail dependency to stop 10 Hz trail rebuilds
5. **#2** -- Decouple position interpolation (biggest single perf win but more invasive)
6. **#3** -- Add clustering (best UX improvement at low zoom)
7. **#6** -- Viewport-based initial load (network optimization)
8. **#8** -- Incremental updates (nice-to-have, depends on MapLibre version)

## Current Architecture Strengths (Keep These)

- **Position bbox filtering**: Backend `query_latest_positions()` with `ST_Within`
  is already efficient. Frontend sends viewport bounds.
- **maxFeatures cap**: `mapStore.maxFeatures = 2000` prevents unbounded growth.
- **Zoom-based position filtering**: Military-only at low zoom, tiered density
  limits (500/1000/3000 by zoom level).
- **Interesting vessel filter**: Backend `is_interesting_vessel()` filters AIS
  noise before it reaches the frontend.
- **Age-based opacity**: Data-driven paint expression uses `age_minutes` property
  rather than creating/destroying layers.
- **Heatmap at low zoom**: Automatically transitions from heatmap (maxzoom 8) to
  individual points at higher zoom.
- **Ground plane hiding**: Configurable filter to hide stationary aircraft.
- **Positions use `replacePositions()`**: Stale entities are removed each poll
  cycle, preventing zombie markers.
