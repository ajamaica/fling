use fling_cli::watcher::{
    Decision, Readiness, WatchState, parse_service_instances, poll_interval_from, retry_delay_from,
    session_environment, shortcut_gameid_from_vdf, steam_session_environment_for_pids,
};

#[test]
fn service_keys_distinguish_instances_and_base_pids() {
    let services = parse_service_instances(
        "com.steampowered.App42 991 owner\n\
         com.steampowered.App42.Instance7 992 owner\n\
         com.steampowered.App42.Instance8 993 owner\n\
         com.steampowered.App420 994 owner\n",
    );
    let keys: Vec<_> = services
        .iter()
        .map(|service| service.key.as_str())
        .collect();
    assert_eq!(keys, ["42:7", "42:8", "420:994"]);
}

#[test]
fn borrows_only_the_safe_steam_session_environment() {
    let values = session_environment(
        b"DISPLAY=:1\0WAYLAND_DISPLAY=wayland-0\0HOME=/attacker\0XAUTHORITY=/xauth\0\
          XDG_RUNTIME_DIR=/run/user/1000\0DBUS_SESSION_BUS_ADDRESS=unix:path=/bus\0BAD\0",
    );
    assert_eq!(values.len(), 5);
    assert_eq!(values.get("DISPLAY").map(String::as_str), Some(":1"));
    assert!(!values.contains_key("HOME"));
}

#[test]
fn unavailable_falls_back_once_at_limit_and_diagnostics_are_one_time() {
    let mut state = WatchState::new(3);
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Waiting),
        Decision::Waiting(true)
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Waiting),
        Decision::Waiting(false)
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Unavailable),
        Decision::Unavailable(true)
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Unavailable),
        Decision::Unavailable(false)
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Unavailable),
        Decision::LaunchFallback
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Ready),
        Decision::None
    );
}

#[test]
fn guards_skip_duplicates_and_retire_per_instance_state() {
    let mut state = WatchState::new(2);
    assert_eq!(
        state.observe("42:6", false, false, Readiness::Ready),
        Decision::None
    );
    assert_eq!(
        state.observe("42:8", true, true, Readiness::Ready),
        Decision::AlreadyRunning
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Ready),
        Decision::LaunchReady
    );
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Ready),
        Decision::None
    );
    state.retire_except(std::iter::empty::<&str>());
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Ready),
        Decision::LaunchReady
    );
}

#[test]
fn one_ready_launch_is_claimed_across_sibling_instances_per_poll() {
    let mut state = WatchState::new(2);
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Ready),
        Decision::LaunchReady
    );
    assert_eq!(
        state.observe("42:8", true, false, Readiness::Ready),
        Decision::None
    );

    state.retire_except(std::iter::empty::<&str>());
    assert_eq!(
        state.observe("42:9", true, false, Readiness::Ready),
        Decision::LaunchReady
    );
}

#[test]
fn one_fallback_launch_is_claimed_across_sibling_instances_per_poll() {
    let mut state = WatchState::new(1);
    assert_eq!(
        state.observe("42:7", true, false, Readiness::Unavailable),
        Decision::LaunchFallback
    );
    assert_eq!(
        state.observe("42:8", true, false, Readiness::Unavailable),
        Decision::None
    );

    state.retire_except(std::iter::empty::<&str>());
    assert_eq!(
        state.observe("42:9", true, false, Readiness::Unavailable),
        Decision::LaunchFallback
    );
}

#[test]
fn parses_matching_steam_shortcut_gameid() {
    let appid = 0x89ab_cdef_u32;
    let mut vdf = b"\x00\x01name\x00other\x00\x02appid\x00".to_vec();
    vdf.extend(appid.to_le_bytes());
    vdf.extend(b"\x01Exe\x00fling run 42\x00\x08\x08");
    assert_eq!(
        shortcut_gameid_from_vdf(&vdf, 42),
        Some(((appid as u64) << 32) | 0x0200_0000)
    );
    assert_eq!(shortcut_gameid_from_vdf(&vdf, 43), None);
}

#[test]
fn shortcut_appid_requires_a_numeric_token_boundary() {
    let shortcut_appid = 123_u32;
    let mut vdf = b"\x02appid\x00".to_vec();
    vdf.extend(shortcut_appid.to_le_bytes());
    vdf.extend(b"\x01Exe\x00fling run 420\x00\x08");

    assert_eq!(shortcut_gameid_from_vdf(&vdf, 42), None);
}

#[test]
fn steam_session_uses_first_usable_environment_across_all_pids() {
    let temp = tempfile::tempdir().expect("tempdir");
    let proc_root = temp.path().join("proc");
    std::fs::create_dir_all(proc_root.join("11")).expect("irrelevant pid");
    std::fs::write(proc_root.join("11/environ"), b"HOME=/irrelevant\0").expect("environment");
    std::fs::create_dir_all(proc_root.join("22")).expect("usable pid");
    std::fs::write(proc_root.join("22/environ"), b"DISPLAY=:22\0").expect("environment");
    let config = fling_cli::config::Config {
        home: temp.path().join("home"),
        trainers: temp.path().join("trainers"),
        steam_root: temp.path().join("steam"),
        proc_root,
    };

    assert_eq!(
        steam_session_environment_for_pids(&config, b"11\n22\n")
            .get("DISPLAY")
            .map(String::as_str),
        Some(":22")
    );
}

#[test]
fn watcher_timing_values_are_safe_for_duration_conversion() {
    for invalid in [
        None,
        Some("invalid"),
        Some("NaN"),
        Some("inf"),
        Some("-1"),
        Some("1e300"),
    ] {
        assert_eq!(retry_delay_from(invalid), 5.0);
        assert_eq!(poll_interval_from(invalid), 5.0);
    }
    assert_eq!(retry_delay_from(Some("0")), 0.0);
    assert_eq!(poll_interval_from(Some("0")), 5.0);
    assert_eq!(retry_delay_from(Some("0.25")), 0.25);
    assert_eq!(poll_interval_from(Some("0.25")), 0.25);
    assert!(std::time::Duration::try_from_secs_f64(retry_delay_from(Some("1e300"))).is_ok());
    assert!(std::time::Duration::try_from_secs_f64(poll_interval_from(Some("1e300"))).is_ok());
}
