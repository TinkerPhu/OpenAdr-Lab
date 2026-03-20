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
pub struct QuantitySeries {
    pub samples: Vec<(DateTime<Utc>, f64)>,
    pub quantity: Quantity,
    pub unit: Unit,
    pub interpolation: Interpolation,
}

impl QuantitySeries {
    pub fn empty(quantity: Quantity, unit: Unit, interpolation: Interpolation) -> Self {
        Self {
            samples: Vec::new(),
            quantity,
            unit,
            interpolation,
        }
    }
}
