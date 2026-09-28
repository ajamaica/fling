use fling_cli::{
    config::Config,
    wemod::{self, Choice},
};
use std::{fs, path::Path};

fn fixture() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = temp.path().join("home");
    let steam = home.join("Steam");
    fs::create_dir_all(steam.join("steamapps/compatdata/42/pfx")).expect("prefix");
    fs::write(
        steam.join("steamapps/appmanifest_42.acf"),
        r#""appid" "42" "name" "Answer" "installdir" "Answer""#,
    )
    .expect("manifest");
    let proc_root = temp.path().join("proc");
    fs::create_dir_all(&proc_root).expect("proc");
    let config = Config {
        trainers: home.join("Trainers"),
        home,
        steam_root: steam,
        proc_root,
    };
    (temp, config)
}

fn app(root: &Path, version: &str) {
    let dir = root.join(format!("app-{version}"));
    fs::create_dir_all(&dir).expect("app dir");
    fs::write(dir.join("WeMod.exe"), b"MZ").expect("exe");
}

#[test]
fn per_game_choices_persist_and_default_to_fling() {
    let (_temp, config) = fixture();
    assert_eq!(wemod::choice(&config, 42), Choice::Fling);
    wemod::set_choice(&config, 42, Choice::Wemod).expect("wemod");
    wemod::set_choice(&config, 7, Choice::Both).expect("both");
    assert_eq!(wemod::choice(&config, 42), Choice::Wemod);
    assert_eq!(wemod::choice(&config, 7), Choice::Both);
    assert_eq!(
        wemod::enabled_appids(&config)
            .into_iter()
            .collect::<Vec<_>>(),
        [7, 42]
    );
    wemod::set_choice(&config, 42, Choice::Fling).expect("fling");
    assert_eq!(wemod::choice(&config, 42), Choice::Fling);
    assert_eq!(wemod::choice(&config, 7), Choice::Both);
}

#[test]
fn prefers_newest_app_in_game_prefix_over_shared_copy() {
    let (_temp, config) = fixture();
    let local = config
        .steam_root
        .join("steamapps/compatdata/42/pfx/drive_c/users/steamuser/AppData/Local/WeMod");
    assert_eq!(wemod::find_exe(&config, 42), None);
    app(&wemod::shared_dir(&config), "1.0.0");
    assert_eq!(
        wemod::find_exe(&config, 42),
        Some(wemod::shared_dir(&config).join("app-1.0.0/WeMod.exe"))
    );
    app(&local, "9.9.0");
    app(&local, "10.2.1");
    fs::create_dir_all(local.join("app-11.0.0")).expect("incomplete app");
    fs::create_dir_all(local.join("packages")).expect("packages");
    assert_eq!(
        wemod::find_exe(&config, 42),
        Some(local.join("app-10.2.1/WeMod.exe"))
    );
}

#[test]
fn plan_starts_the_chosen_trainers() {
    let (_temp, config) = fixture();
    let trainer = config.trainers.join("42 - Answer/Trainer.exe");
    fs::create_dir_all(trainer.parent().expect("parent")).expect("trainer dir");
    fs::write(&trainer, b"MZ").expect("trainer");
    let exe = wemod::shared_dir(&config).join("app-1.0.0/WeMod.exe");
    app(&wemod::shared_dir(&config), "1.0.0");

    let plan = wemod::plan(&config, 42);
    assert_eq!((plan.trainer.as_ref(), plan.wemod), (Some(&trainer), None));

    wemod::set_choice(&config, 42, Choice::Both).expect("both");
    let plan = wemod::plan(&config, 42);
    assert_eq!(
        (plan.trainer.as_ref(), plan.wemod.as_ref()),
        (Some(&trainer), Some(&exe))
    );

    wemod::set_choice(&config, 42, Choice::Wemod).expect("wemod");
    let plan = wemod::plan(&config, 42);
    assert_eq!((plan.trainer, plan.wemod.as_ref()), (None, Some(&exe)));

    fs::remove_dir_all(wemod::shared_dir(&config)).expect("remove wemod");
    assert!(wemod::plan(&config, 42).is_empty());
}

#[test]
fn running_matches_only_this_games_wemod() {
    let (_temp, config) = fixture();
    let process = |pid: &str, cmd: &[u8], env: &[u8]| {
        let dir = config.proc_root.join(pid);
        fs::create_dir_all(&dir).expect("pid");
        fs::write(dir.join("cmdline"), cmd).expect("cmdline");
        fs::write(dir.join("environ"), env).expect("environ");
    };
    process(
        "10",
        b"C:\\WeMod\\app-1\\WeMod.exe\0",
        b"STEAM_COMPAT_APP_ID=7\0",
    );
    process(
        "11",
        b"/games/Answer/Answer.exe\0",
        b"STEAM_COMPAT_APP_ID=42\0",
    );
    assert!(!wemod::running(&config, 42));
    process(
        "12",
        b"C:\\WeMod\\app-1\\WeMod.exe\0--type=gpu\0",
        b"STEAM_COMPAT_APP_ID=42\0",
    );
    assert!(wemod::running(&config, 42));
    fs::remove_dir_all(config.proc_root.join("12")).expect("remove");
    process(
        "13",
        b"wine\0/home/u/Steam/steamapps/compatdata/42/pfx/drive_c/users/steamuser/AppData/Local/WeMod/app-1/WeMod.exe\0",
        b"",
    );
    assert!(wemod::running(&config, 42));
}

fn add_game(config: &Config, appid: u32) {
    fs::create_dir_all(
        config
            .steam_root
            .join(format!("steamapps/compatdata/{appid}/pfx")),
    )
    .expect("prefix");
    fs::write(
        config
            .steam_root
            .join(format!("steamapps/appmanifest_{appid}.acf")),
        format!(r#""appid" "{appid}" "name" "Game {appid}" "installdir" "Game{appid}""#),
    )
    .expect("manifest");
}

fn roaming(config: &Config, appid: u32) -> std::path::PathBuf {
    config.steam_root.join(format!(
        "steamapps/compatdata/{appid}/pfx/drive_c/users/steamuser/AppData/Roaming/WeMod"
    ))
}

#[test]
fn first_games_sign_in_is_shared_with_later_games() {
    let (_temp, config) = fixture();
    add_game(&config, 43);
    let shared = wemod::profile_dir(&config);

    fs::create_dir_all(roaming(&config, 42).join("Local Storage")).expect("profile");
    fs::write(
        roaming(&config, 42).join("Local Storage/session"),
        b"signed-in",
    )
    .expect("session");
    wemod::share_profile(&config, 42).expect("share first");
    assert_eq!(fs::read_link(roaming(&config, 42)).expect("link"), shared);
    assert_eq!(
        fs::read(shared.join("Local Storage/session")).expect("adopted"),
        b"signed-in"
    );
    wemod::share_profile(&config, 42).expect("idempotent");

    // A later game's own (signed-out) profile is kept as a backup, not used.
    fs::create_dir_all(roaming(&config, 43)).expect("profile");
    fs::write(roaming(&config, 43).join("stale"), b"x").expect("stale");
    wemod::share_profile(&config, 43).expect("share second");
    assert_eq!(fs::read_link(roaming(&config, 43)).expect("link"), shared);
    assert!(shared.join("Local Storage/session").is_file());
    assert!(!shared.join("stale").exists());
    let backups: Vec<_> = fs::read_dir(roaming(&config, 43).parent().expect("parent"))
        .expect("roaming")
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("WeMod.fling-backup-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert!(backups[0].path().join("stale").is_file());

    // A game that never ran WeMod is simply linked.
    add_game(&config, 44);
    wemod::share_profile(&config, 44).expect("share fresh");
    assert_eq!(fs::read_link(roaming(&config, 44)).expect("link"), shared);
}

#[test]
fn one_install_serves_every_game() {
    let (_temp, config) = fixture();
    add_game(&config, 43);
    let local = config
        .steam_root
        .join("steamapps/compatdata/42/pfx/drive_c/users/steamuser/AppData/Local/WeMod");
    app(&local, "9.1.0");
    fs::write(local.join("Update.exe"), b"MZ").expect("update");
    assert_eq!(wemod::find_exe(&config, 43), None);

    assert!(wemod::import_install(&config, 42).expect("import"));
    assert!(!wemod::import_install(&config, 42).expect("nothing new"));
    let shared_exe = wemod::shared_dir(&config).join("app-9.1.0/WeMod.exe");
    assert!(wemod::shared_dir(&config).join("Update.exe").is_file());
    assert_eq!(wemod::find_exe(&config, 43), Some(shared_exe.clone()));
    assert_eq!(wemod::find_exe(&config, 42), Some(shared_exe));

    // A newer install in one prefix still wins until it is imported.
    app(&local, "9.2.0");
    assert_eq!(
        wemod::find_exe(&config, 42),
        Some(local.join("app-9.2.0/WeMod.exe"))
    );
}

#[test]
fn running_detects_shared_install_by_wineprefix() {
    let (_temp, config) = fixture();
    let dir = config.proc_root.join("20");
    fs::create_dir_all(&dir).expect("pid");
    fs::write(
        dir.join("cmdline"),
        b"Z:\\home\\u\\.local\\share\\fling\\wemod\\app-9.1.0\\WeMod.exe\0",
    )
    .expect("cmdline");
    fs::write(
        dir.join("environ"),
        b"WINEPREFIX=/home/u/Steam/steamapps/compatdata/42/pfx/\0",
    )
    .expect("environ");
    assert!(wemod::running(&config, 42));
    assert!(!wemod::running(&config, 4));
}
