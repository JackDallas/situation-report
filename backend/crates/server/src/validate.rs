/// Clamp a zoom value to the valid range [0, 22].
pub fn clamp_zoom(z: f64) -> f64 {
    z.clamp(0.0, 22.0)
}

/// Clamp latitude to [-90, 90].
pub fn clamp_lat(lat: f64) -> f64 {
    lat.clamp(-90.0, 90.0)
}

/// Clamp longitude to [-180, 180].
pub fn clamp_lon(lon: f64) -> f64 {
    lon.clamp(-180.0, 180.0)
}

/// Clamp hours to a reasonable range [0, 8760] (1 year).
pub fn clamp_hours(h: f64) -> f64 {
    h.clamp(0.0, 8760.0)
}

/// Clamp radius_km to [0, 40075] (Earth circumference).
pub fn clamp_radius_km(r: f64) -> f64 {
    r.clamp(0.0, 40075.0)
}

/// Clamp a limit to [0, max].
pub fn clamp_limit(limit: i64, max: i64) -> i64 {
    limit.clamp(0, max)
}
