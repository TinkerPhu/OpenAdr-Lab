use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerObjective {
    #[default]
    MinCost,
    MinGhg,
    MinGrid,
    MinImport,
    MaxRevenue,
    Custom,
}

impl PlannerObjective {
    fn as_str(self) -> &'static str {
        match self {
            Self::MinCost => "min_cost",
            Self::MinGhg => "min_ghg",
            Self::MinGrid => "min_grid",
            Self::MinImport => "min_import",
            Self::MaxRevenue => "max_revenue",
            Self::Custom => "custom",
        }
    }
}

impl Serialize for PlannerObjective {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str((*self).as_str())
    }
}

impl<'de> Deserialize<'de> for PlannerObjective {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "min_cost" => Ok(Self::MinCost),
            "min_ghg" => Ok(Self::MinGhg),
            "min_grid" => Ok(Self::MinGrid),
            "min_import" => Ok(Self::MinImport),
            "max_revenue" => Ok(Self::MaxRevenue),
            "custom" => Ok(Self::Custom),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "min_cost",
                    "min_ghg",
                    "min_grid",
                    "min_import",
                    "max_revenue",
                    "custom",
                ],
            )),
        }
    }
}
