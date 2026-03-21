use chrono::{DateTime, Utc};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interpolation {
    Linear,
    Step,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quantity {
    Power,
    Energy,
    StateOfCharge,
    Temperature,
    Irradiance,
    Tariff,
    Co2Intensity,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    Kilowatt,
    KilowattHour,
    Percent,
    Celsius,
    WattsPerSquareMeter,
    EuroPerKilowattHour,
    GramsPerKilowattHour,
}

#[derive(Debug, Clone)]
pub struct QuantityTimeline {
    pub samples: Vec<(DateTime<Utc>, f64)>,
    pub quantity: Quantity,
    pub unit: Unit,
    pub interpolation: Interpolation,
}

impl QuantityTimeline {
    pub fn empty(quantity: Quantity, unit: Unit, interpolation: Interpolation) -> Self {
        Self {
            samples: Vec::new(),
            quantity,
            unit,
            interpolation,
        }
    }

    /// Verify that all timestamps are strictly ascending.
    pub fn is_ascending(&self) -> bool {
        self.samples.windows(2).all(|w| w[0].0 < w[1].0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    #[test]
    fn empty_series_has_no_samples() {
        let s = QuantityTimeline::empty(Quantity::Power, Unit::Kilowatt, Interpolation::Linear);
        assert!(s.samples.is_empty());
    }

    #[test]
    fn empty_series_is_trivially_ascending() {
        let s = QuantityTimeline::empty(Quantity::Power, Unit::Kilowatt, Interpolation::Linear);
        assert!(s.is_ascending());
    }

    #[test]
    fn non_empty_series_ascending_check() {
        let now = Utc::now();
        let s = QuantityTimeline {
            samples: vec![(now, 1.0), (now + Duration::seconds(60), 2.0)],
            quantity: Quantity::Power,
            unit: Unit::Kilowatt,
            interpolation: Interpolation::Linear,
        };
        assert!(s.is_ascending());
    }

    #[test]
    fn series_with_same_timestamp_not_ascending() {
        let now = Utc::now();
        let s = QuantityTimeline {
            samples: vec![(now, 1.0), (now, 2.0)],
            quantity: Quantity::Power,
            unit: Unit::Kilowatt,
            interpolation: Interpolation::Linear,
        };
        assert!(!s.is_ascending());
    }
}
