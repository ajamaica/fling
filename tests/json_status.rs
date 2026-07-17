use fling_cli::{
    config::Config,
    json_api::{steam_environment_active, steam_environment_active_for_pids},
};
use std::fs;

fn config() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = Config {
        home: temp.path().join("home"),
        trainers: temp.path().join("trainers"),
        steam_root: temp.path().join("steam"),
        proc_root: temp.path().join("proc"),
    };
    fs::create_dir(&config.proc_root).expect("proc root");
    (temp, config)
}

#[test]
fn detects_exact_launcher_service_environment_entry() {
    let (_temp, config) = config();
    fs::create_dir(config.proc_root.join("71")).expect("pid");
    fs::write(
        config.proc_root.join("71/environ"),
        b"DISPLAY=:0\0STEAM_COMPAT_LAUNCHER_SERVICE=proton\0",
    )
    .expect("environment");
    assert!(steam_environment_active(&config, 71));
}

#[test]
fn rejects_absent_partial_and_unreadable_environment() {
    let (_temp, config) = config();
    for (pid, contents) in [
        (72, b"DISPLAY=:0\0".as_slice()),
        (73, b"X=STEAM_COMPAT_LAUNCHER_SERVICE=proton\0".as_slice()),
        (
            74,
            b"STEAM_COMPAT_LAUNCHER_SERVICE=proton-extra\0".as_slice(),
        ),
    ] {
        fs::create_dir(config.proc_root.join(pid.to_string())).expect("pid");
        fs::write(config.proc_root.join(format!("{pid}/environ")), contents).expect("environ");
        assert!(!steam_environment_active(&config, pid));
    }
    fs::create_dir(config.proc_root.join("75")).expect("missing environ pid");
    fs::create_dir_all(config.proc_root.join("76/environ")).expect("unreadable environment kind");
    assert!(!steam_environment_active(&config, 75));
    assert!(!steam_environment_active(&config, 76));
}

#[test]
fn status_environment_checks_every_steam_pid() {
    let (_temp, config) = config();
    fs::create_dir(config.proc_root.join("81")).expect("irrelevant pid");
    fs::write(config.proc_root.join("81/environ"), b"DISPLAY=:0\0").expect("environment");
    fs::create_dir(config.proc_root.join("82")).expect("active pid");
    fs::write(
        config.proc_root.join("82/environ"),
        b"STEAM_COMPAT_LAUNCHER_SERVICE=proton\0",
    )
    .expect("environment");

    assert!(steam_environment_active_for_pids(&config, b"81\n82\n"));
}
