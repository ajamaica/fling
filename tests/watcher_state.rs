use fling_cli::watcher::{
    Decision, Readiness, WatchState, parse_service_instances, session_environment,
    shortcut_gameid_from_vdf,
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
