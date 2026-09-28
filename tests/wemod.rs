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
