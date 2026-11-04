//! PV array parameters of the weather-sourced forecast: geometry, snow model, electrical rating.
//! Pure data, declared once in the VEN profile; read by `solar` and `pv_snow`.

/// PV array geometry, independent of electrical rating: geo-position, panel
/// tilt (0°=horizontal, 90°=vertical), and panel azimuth (compass bearing
/// the panel faces, 0°=N/90°=E/180°=S/270°=W).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvArrayGeometry {
    pub location: crate::weather::GeoPosition,
    pub tilt_deg: f64,
    pub azimuth_deg: f64,
}

/// Snow-cover model parameters — see `entities::pv_snow` for the state
/// machine these drive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvSnowParams {
    /// New snowfall this hour above this amount triggers full coverage.
    pub snowfall_trigger_cm: f64,
    /// Temperature above which a covered panel is assumed to self-clear
    /// within the hour.
    pub clear_threshold_c: f64,
    /// Output fraction while covered — usually 0.0.
    pub covered_output_fraction: f64,
}

impl Default for PvSnowParams {
    fn default() -> Self {
        Self {
            snowfall_trigger_cm: 0.2,
            clear_threshold_c: 1.5,
            covered_output_fraction: 0.0,
        }
    }
}

/// Full parameter set for the weather-sourced PV forecast
/// (`entities::solar::forecast_ac_kw`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PvForecastParams {
    /// DC nameplate rating at STC (1000 W/m², 25°C cell temp).
    pub rated_kwp: f64,
    pub geometry: PvArrayGeometry,
    /// System losses: inverter conversion, wiring, soiling, mismatch —
    /// everything NOT already captured by rated_kwp (module efficiency).
    pub performance_ratio: f64,
    /// Percent power change per °C above 25°C cell temperature (negative).
    pub temp_coeff_pct_per_c: f64,
    /// Nominal Operating Cell Temperature, for the cell-temperature model.
    pub noct_c: f64,
    /// Inverter AC clipping cap, if the AC rating is below rated_kwp.
    pub ac_limit_kw: Option<f64>,
    pub snow: PvSnowParams,
}
