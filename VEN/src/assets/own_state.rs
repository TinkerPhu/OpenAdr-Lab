//! Where each asset's state sits inside `AssetState`, declared once.
//!
//! The `Asset` trait takes `&AssetState` — the enum — because `SimState`
//! stores and serializes a heterogeneous roster. So every method of every
//! asset opened the same way:
//!
//! ```ignore
//! let AssetState::Ev(s) = state else { unreachable!("EvCharger/state mismatch") };
//! self.step_inner(s, setpoint_kw, dt)
//! ```
//!
//! 38 of those arms across six asset kinds, each its own panic site, each
//! reporting only which variant it *wanted*. Config and state being parallel
//! enums the trait cannot pair is a real type-safety hole — closing it needs
//! an associated-state redesign of `Asset`, which is its own piece of work.
//! What this module does instead is make the pairing **declared once and
//! checked once**: `OwnState` is the declaration, `own` is the check.
//!
//! The panic is kept rather than softened to an `Option`: with
//! `persist::load_with_params` now discarding a persisted state of the wrong
//! kind, a mismatch here means a roster this process built inconsistently,
//! which is a bug in this binary and not a condition any caller can handle.
//! But it now names what it actually got, which `unreachable!("…/state
//! mismatch")` never did.

use super::{
    AssetState, BaseLoadState, BatteryState, EvState, GridState, HeaterState, PvState,
    ShiftableLoadState,
};

/// One asset-state type, and where it lives in `AssetState`.
pub trait OwnState: Sized {
    /// The variant name, for the panic message in `own`.
    const VARIANT: &'static str;
    /// Borrow this type out of the enum, or `None` if it holds another kind.
    fn borrow(state: &AssetState) -> Option<&Self>;
    /// As `borrow`, for the methods that write state back (`reset`).
    fn borrow_mut(state: &mut AssetState) -> Option<&mut Self>;
}

/// An asset's own state, out of the roster's enum.
///
/// # Panics
/// When `state` holds a different asset kind — see the module doc for why
/// that is a panic and not a `Result`.
pub fn own<S: OwnState>(state: &AssetState) -> &S {
    S::borrow(state).unwrap_or_else(|| {
        panic!(
            "asset state mismatch: expected {}, roster holds {}",
            S::VARIANT,
            variant_name(state)
        )
    })
}

/// An asset's own state, mutably.
///
/// # Panics
/// As `own`.
pub fn own_mut<S: OwnState>(state: &mut AssetState) -> &mut S {
    // The borrow checker will not let the `unwrap_or_else` closure read
    // `state` again after `borrow_mut` took it, so the variant name is read
    // first and only used on the failing path.
    let held = variant_name(state);
    S::borrow_mut(state).unwrap_or_else(|| {
        panic!(
            "asset state mismatch: expected {}, roster holds {held}",
            S::VARIANT,
        )
    })
}

/// The variant `state` actually holds — the half of the diagnosis the old
/// per-asset panics left out.
pub fn variant_name(state: &AssetState) -> &'static str {
    match state {
        AssetState::Battery(_) => "Battery",
        AssetState::Ev(_) => "Ev",
        AssetState::Heater(_) => "Heater",
        AssetState::Pv(_) => "Pv",
        AssetState::BaseLoad(_) => "BaseLoad",
        AssetState::ShiftableLoad(_) => "ShiftableLoad",
        AssetState::Grid(_) => "Grid",
    }
}

/// One `OwnState` impl per variant. A macro because the six are identical but
/// for two names, and because adding a variant to `AssetState` without a row
/// here should be the only thing a new asset kind has to remember.
macro_rules! own_state {
    ($($state:ty => $variant:ident),+ $(,)?) => {
        $(
            impl OwnState for $state {
                const VARIANT: &'static str = stringify!($variant);
                fn borrow(state: &AssetState) -> Option<&Self> {
                    match state {
                        AssetState::$variant(s) => Some(s),
                        _ => None,
                    }
                }
                fn borrow_mut(state: &mut AssetState) -> Option<&mut Self> {
                    match state {
                        AssetState::$variant(s) => Some(s),
                        _ => None,
                    }
                }
            }
        )+
    };
}

own_state! {
    BatteryState => Battery,
    EvState => Ev,
    HeaterState => Heater,
    PvState => Pv,
    BaseLoadState => BaseLoad,
    ShiftableLoadState => ShiftableLoad,
    GridState => Grid,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn battery_state() -> AssetState {
        AssetState::Battery(BatteryState {
            soc: 0.42,
            actual_power_kw: 1.5,
        })
    }

    #[test]
    fn own_borrows_the_matching_variant() {
        let state = battery_state();
        let s: &BatteryState = own(&state);
        assert!((s.soc - 0.42).abs() < 1e-9);
    }

    #[test]
    fn borrow_returns_none_for_another_kind() {
        assert!(EvState::borrow(&battery_state()).is_none());
    }

    /// The old per-asset panics said only which variant was wanted. Half of a
    /// mismatch's diagnosis is which one is actually there.
    #[test]
    #[should_panic(expected = "expected Ev, roster holds Battery")]
    fn own_panics_naming_both_sides() {
        let state = battery_state();
        let _: &EvState = own(&state);
    }

    #[test]
    fn own_mut_borrows_the_matching_variant_mutably() {
        let mut state = battery_state();
        let s: &mut BatteryState = own_mut(&mut state);
        s.soc = 0.9;
        assert!(matches!(state, AssetState::Battery(ref b) if (b.soc - 0.9).abs() < 1e-9));
    }

    #[test]
    #[should_panic(expected = "expected Ev, roster holds Battery")]
    fn own_mut_panics_naming_both_sides() {
        let mut state = battery_state();
        let _: &mut EvState = own_mut(&mut state);
    }

    #[test]
    fn variant_name_covers_every_variant() {
        // Compile-time exhaustiveness is `variant_name`'s own match; this
        // pins the strings the panic message is built from.
        assert_eq!(variant_name(&battery_state()), "Battery");
        assert_eq!(BatteryState::VARIANT, "Battery");
        assert_eq!(EvState::VARIANT, "Ev");
        assert_eq!(HeaterState::VARIANT, "Heater");
        assert_eq!(PvState::VARIANT, "Pv");
        assert_eq!(BaseLoadState::VARIANT, "BaseLoad");
        assert_eq!(ShiftableLoadState::VARIANT, "ShiftableLoad");
        assert_eq!(GridState::VARIANT, "Grid");
    }
}
