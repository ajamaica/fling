use fling_cli::game_profiles::{ELDEN_RING_APPID, for_appid};

#[test]
fn elden_ring_profile_requires_delayed_offline_windowed_launch() {
    let profile = for_appid(ELDEN_RING_APPID).expect("Elden Ring profile");

    assert_eq!(profile.trainer_launch_delay_seconds, 90);
    assert_eq!(
        profile.trainer_instructions,
        [
            "Use Windowed mode before activating the trainer.",
            "Launch without Easy Anti-Cheat (EAC) and stay offline.",
        ]
    );
}

#[test]
fn ordinary_games_have_no_special_profile() {
    assert!(for_appid(367520).is_none());
}
