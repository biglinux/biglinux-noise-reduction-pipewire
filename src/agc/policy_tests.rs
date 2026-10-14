use super::*;

fn feature(p: f64) -> Features {
    Features {
        speech_dbfs: -55.0,
        noise_dbfs: -100.0,
        raw_clip_fraction: 0.0,
        speech_probability: p,
        snr_db: 45.0,
        muted: false,
        far_end_only: false,
        unreliable_double_talk: false,
        discontinuity: false,
    }
}

#[test]
fn physical_ceiling_is_reachable_not_deadband_below_it() {
    let c = Config {
        target_speech_dbfs: -12.0,
        max_boost_db: 28.627_275_283_179_745,
        rise_ceil_db_per_s: 8.0,
        ..Config::default()
    };
    let mut p = GainPolicy::new(None, c).unwrap();
    for _ in 0..4000 {
        p.update(&feature(1.0), 0.05).unwrap();
    }
    assert!((p.current_gain_db() - c.max_boost_db).abs() < 1e-8);
}

#[test]
fn untrusted_large_peak_cuts_immediately_with_bounded_slew() {
    let c = Config {
        min_digital_db: -50.0,
        fall_ceil_db_per_s: 14.0,
        ..Config::default()
    };
    let mut p = GainPolicy::new(None, c).unwrap();
    let mut previous = 0.0;
    for _ in 0..40 {
        let d = p
            .update_with_peak_guard(&feature(0.0), 0.05, 30.0, -3.0)
            .unwrap();
        assert!(d.digital_gain_db <= previous);
        assert!(previous - d.digital_gain_db <= 14.0 * 0.05 + 1e-8);
        assert!(d.hardware_request.is_none());
        previous = d.digital_gain_db;
    }
    assert!(previous < -20.0, "fast reduction, got {previous}");
}

#[test]
fn peak_guard_never_authorizes_boost_or_overrides_manual_hold() {
    let mut p = GainPolicy::new(None, Config::default()).unwrap();
    for _ in 0..400 {
        p.update_with_peak_guard(&feature(0.0), 0.05, -60.0, -3.0)
            .unwrap();
    }
    assert_eq!(p.current_gain_db(), 0.0);
    p.user_changed_gain();
    let d = p
        .update_with_peak_guard(&feature(1.0), 0.05, 30.0, -3.0)
        .unwrap();
    assert_eq!(d.state, "manual_hold");
    assert_eq!(d.digital_gain_db, 0.0);
}

#[test]
fn peak_guard_honors_mute_and_discontinuity() {
    for f in [
        Features {
            muted: true,
            ..feature(1.0)
        },
        Features {
            discontinuity: true,
            ..feature(1.0)
        },
    ] {
        let mut p = GainPolicy::new(None, Config::default()).unwrap();
        let d = p.update_with_peak_guard(&f, 0.05, 40.0, -3.0).unwrap();
        assert_eq!(d.digital_gain_db, 0.0);
        assert!(matches!(d.state, "muted" | "settling"));
    }
}

#[test]
fn reconciliation_preserves_manual_ownership_and_rejects_nonfinite() {
    let mut p = GainPolicy::new(None, Config::default()).unwrap();
    p.user_changed_gain();
    p.synchronize_external_gain(3.0).unwrap();
    assert_eq!(p.update(&feature(1.0), 0.05).unwrap().state, "manual_hold");
    assert_eq!(p.current_gain_db(), 3.0);
    for bad in [f64::NAN, f64::INFINITY, 100.0, -100.0] {
        assert!(p.synchronize_external_gain(bad).is_err());
        assert_eq!(p.current_gain_db(), 3.0);
    }
}

#[test]
fn reconciliation_cannot_bypass_hardware_ack_protocol() {
    let h = Hardware {
        device_epoch: "epoch".into(),
        gain_db: 0.0,
        min_db: 0.0,
        max_db: 30.0,
        step_db: 1.5,
        verified_before_adc: true,
        writable_and_readable: true,
        raw_clip_meter_verified: true,
    };
    let mut p = GainPolicy::new(Some(h), Config::default()).unwrap();
    assert!(p.synchronize_external_gain(1.0).is_err());
}

#[test]
fn invalid_peak_guard_does_not_mutate_policy() {
    let mut p = GainPolicy::new(None, Config::default()).unwrap();
    assert!(
        p.update_with_peak_guard(&feature(1.0), 0.05, f64::NAN, -3.0)
            .is_err()
    );
    assert!(
        p.update_with_peak_guard(&feature(1.0), 0.05, -20.0, f64::INFINITY)
            .is_err()
    );
    assert_eq!(p.current_gain_db(), 0.0);
}
