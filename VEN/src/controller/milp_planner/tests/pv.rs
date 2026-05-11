use super::*;

    // ── PV forecast reflects live irradiance_offset and pv_alpha ─────────────

    /// Return midnight so natural_irradiance_at() = 0, isolating the offset term.
    fn fixed_midnight() -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 4, 12, 0, 0, 0).unwrap()
    }

    /// Set irradiance_offset and pv_alpha on the PV asset in an existing SimSnapshot.
    fn set_pv_inject(sim: &mut SimSnapshot, offset: f64, alpha: f64) {
        let snap = sim.assets.get_mut("pv").expect("no pv asset in sim");
        snap.values.insert("irradiance_offset".to_string(), offset);
        snap.values.insert("pv_alpha".to_string(), alpha);
    }

    #[test]
    fn pv_irradiance_offset_in_forecast() {
        // Regression: irradiance_offset must project into p_pv_kw.
        // At midnight, natural irradiance = 0. With offset=0.5 and very slow
        // alpha (≈no decay over the horizon), slot 0 must be ≈ 0.5 × rated_kw.
        let now = fixed_midnight();
        let profile = make_profile(); // rated_kw=5.0
        let mut sim = make_snap_from_profile(&profile);
        set_pv_inject(&mut sim, 0.5, 0.001); // slow alpha → offset barely decays

        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);

        // slot 0: seconds_ahead=0 → decayed_offset = 0.5×(0.999)^0 = 0.5
        // p_pv[0] = (0.0 + 0.5).clamp(0,1) × 5.0 = 2.5 kW
        assert!(
            inp.p_pv_kw[0] > 1.0,
            "p_pv_kw[0] should reflect irradiance_offset (got {:.4})",
            inp.p_pv_kw[0]
        );
    }

    #[test]
    fn pv_irradiance_offset_decays_per_step_not_per_second() {
        // Regression guard: with alpha=0.1 (typical), the decay exponent must be
        // the plan-step count (t), NOT raw seconds (t * 300).
        // Buggy formula: 0.9^(1×300) ≈ 5e-14  → slot 1 ≈ 0 kW  (WRONG)
        // Correct formula: 0.9^1 = 0.9         → slot 1 ≈ 2.25 kW (RIGHT)
        let now = fixed_midnight(); // natural=0, isolates offset
        let profile = make_profile(); // rated_kw=5.0, step_s=300
        let mut sim = make_snap_from_profile(&profile);
        set_pv_inject(&mut sim, 0.5, 0.1); // typical alpha=0.1

        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);

        // slot 0: 0.5 × 0.9^0 × 5.0 = 2.5 kW
        // slot 1: 0.5 × 0.9^1 × 5.0 = 2.25 kW (must be clearly non-zero)
        assert!(
            inp.p_pv_kw[1] > 1.0,
            "slot 1 must retain offset with alpha=0.1 (decay per step, not per second), got {:.6}",
            inp.p_pv_kw[1]
        );
        // slot 5: 0.5 × 0.9^5 × 5.0 ≈ 1.476 kW
        assert!(
            inp.p_pv_kw[5] > 0.5,
            "slot 5 must still show partial offset, got {:.6}",
            inp.p_pv_kw[5]
        );
        // Decay is monotonically decreasing (offset fades over horizon)
        assert!(
            inp.p_pv_kw[1] < inp.p_pv_kw[0],
            "slot 1 must be less than slot 0 (offset decaying)"
        );
    }

    #[test]
    fn pv_alpha_faster_decay_in_forecast() {
        // Regression: higher pv_alpha (blend-back speed) must produce lower p_pv_kw
        // at later forecast slots because the offset decays faster.
        // At midnight natural=0, so all forecast power comes from the decaying offset.
        let now = fixed_midnight();
        let profile = make_profile(); // rated_kw=5.0, step_s=300s, 24 slots

        let mut sim_slow = make_snap_from_profile(&profile);
        set_pv_inject(&mut sim_slow, 0.5, 0.001); // slow: 0.1 % per second

        let mut sim_fast = make_snap_from_profile(&profile);
        set_pv_inject(&mut sim_fast, 0.5, 0.05); // fast: 5 % per second

        let inp_slow = build_milp_inputs(
            &sim_slow,
            &TariffTimeSeries::from_snapshots(&[]),
            &no_capacity(),
            &profile,
            now,
            None,
            None,
            &[],
            None,
        );
        let inp_fast = build_milp_inputs(
            &sim_fast,
            &TariffTimeSeries::from_snapshots(&[]),
            &no_capacity(),
            &profile,
            now,
            None,
            None,
            &[],
            None,
        );

        // At slot 3 (900 s ahead at midnight, natural=0):
        //   slow: 0.5 × (0.999)^900 ≈ 0.5 × 0.41 ≈ 2.0 kW
        //   fast: 0.5 × (0.95)^900  ≈ 0.5 × ~0   ≈ 0.0 kW
        let t = 3;
        assert!(
            inp_fast.p_pv_kw[t] < inp_slow.p_pv_kw[t],
            "higher alpha should produce lower p_pv_kw at later slots: \
             fast={:.4} >= slow={:.4}",
            inp_fast.p_pv_kw[t],
            inp_slow.p_pv_kw[t]
        );
    }

    #[test]
    fn pv_zero_offset_matches_sin_model() {
        // Backward compat: when irradiance_offset=0, p_pv_kw must equal the
        // profile's pure sin model (PvConfig::forecast_kw).
        let now = fixed_now(); // 06:00 → natural = 0 at slot 0
        let profile = make_profile(); // rated_kw=5.0, step_s=300s

        // from_profile initialises irradiance_offset=0, pv_alpha=0.1
        let sim = make_snap_from_profile(&profile);

        let inp = bmi(&profile, &sim, &TariffTimeSeries::from_snapshots(&[]), &no_capacity(), now, None, None);

        // Compare every slot against the profile's sin model
        let pv_cfg = profile.pv_config().unwrap();
        for t in 0..inp.n {
            let slot_t = now + Duration::seconds(t as i64 * 300);
            let expected = pv_cfg.forecast_kw(slot_t);
            assert!(
                (inp.p_pv_kw[t] - expected).abs() < 1e-9,
                "slot {t}: zero-offset p_pv_kw should match sin model \
                 (got {:.6}, expected {:.6})",
                inp.p_pv_kw[t],
                expected
            );
        }
    }

