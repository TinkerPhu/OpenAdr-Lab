use serde::{Deserialize, Serialize};

/// Asset type classification (§1.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AssetType {
    Pv,              // photovoltaic producer
    Battery,         // bidirectional storage
    Ev,              // electric vehicle (consumer, storage-like)
    Heater,          // thermal consumer with storage characteristics
    HeatPump,        // thermal consumer with storage characteristics
    WashingMachine,  // batch consumer
    CookingStove,    // heuristic/uncontrollable consumer
    SiteResidual,    // virtual asset: unmodeled site consumption
    GenericConsumer, // fallback
    GenericProducer, // fallback
}

/// Device health and communication status (§1.3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeviceResponsiveness {
    Responsive,   // device confirms setpoints within expected delay
    Degraded,     // device responds but outside expected parameters
    Unresponsive, // device not confirming setpoint changes
    Offline,      // device not communicating at all
}

/// How to handle completion when the last DeadlineTier expires (§1.10).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionPolicy {
    /// Terminate immediately → PARTIAL_COMPLETED if FillPercentage < 1.0.
    Stop,
    /// Keep going, bidding at PostDeadlineComfortBid for priority.
    Continue,
}

/// What triggered a plan recomputation (§1.5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlanTrigger {
    Periodic,         // regular planning cycle (every PlanTimeStep)
    RateChange,       // new PRICE/GHG/EXPORT_PRICE event from VTN
    CapacityChange,   // new capacity limit/reservation from VTN
    Alert,            // emergency/flex alert from VTN
    UserRequest,      // new or modified device session / user request
    AssetStateChange, // device connected/disconnected/failed
}

/// One point on the comfort/value curve (§2.7).
/// MaxMarginalPrice is a priority bid, not the actual price paid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortRate {
    pub fill: f64,               // 0.0..1.0 task completion fraction
    pub max_marginal_price: f64, // max €/kWh the user bids — determines priority
    pub max_marginal_co2: f64,   // max gCO2/kWh user accepts at this fill level
}
