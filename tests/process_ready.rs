use fling_cli::{config::Config, process::game_ready};
use std::{fs, path::Path};
use tempfile::TempDir;

fn fixture() -> (TempDir, Config) {
    let temp = tempfile::tempdir().expect("tempdir");
    let steam = temp.path().join("steam");
    let proc_root = temp.path().join("proc");
    fs::create_dir_all(steam.join("steamapps/common/Real Game")).expect("game dir");
    fs::create_dir_all(&proc_root).expect("proc root");
    fs::write(
        steam.join("steamapps/appmanifest_42.acf"),
        r#""AppState"
{
    "appid" "42"
    "name" "Real Game"
    "installdir" "Real Game"
}"#,
    )
    .expect("manifest");
    let config = Config {
        home: temp.path().join("home"),
        trainers: temp.path().join("trainers"),
        steam_root: steam,
        proc_root,
    };
    (temp, config)
}

fn process(config: &Config, pid: u32, argv: &[&str]) {
    let dir = config.proc_root.join(pid.to_string());
    fs::create_dir(&dir).expect("pid dir");
    fs::write(dir.join("environ"), b"X=1\0STEAM_COMPAT_APP_ID=42\0").expect("environ");
    let mut cmdline = argv.join("\0").into_bytes();
    cmdline.push(0);
    fs::write(dir.join("cmdline"), cmdline).expect("cmdline");
}

#[test]
fn readiness_is_scoped_to_real_game_argv0_and_rejects_helpers() {
    let (_temp, config) = fixture();
    assert_eq!(
        game_ready(&config, 42),
        1,
        "no matching process is not ready"
    );

    process(
        &config,
        1,
        &[
            "C:\\Other\\Real Game Deluxe\\fake.exe",
            "Z:\\Real Game\\real.exe",
        ],
    );
    process(&config, 2, &["Z:\\Real Game\\EADesktop.exe"]);

    let helper_paths = [
        "_CommonRedist/x.exe",
        "redist/x.exe",
        "redistributable/x.exe",
        "__Installer/x.exe",
        "installer/x.exe",
        "installers/x.exe",
        "prerequisites/x.exe",
        "prereqs/x.exe",
        "supportsoftware/x.exe",
        "EasyAntiCheat/x.exe",
        "anticheat/x.exe",
    ];
    for (offset, helper) in helper_paths.iter().enumerate() {
        process(
            &config,
            10 + offset as u32,
            &[&format!("Z:\\Real Game\\{helper}")],
        );
    }
    let helper_names = [
        "cleanup.exe",
        "crashpad_handler.exe",
        "crashreportclient.exe",
        "dxsetup.exe",
        "eappinstaller.exe",
        "eac_launcher.exe",
        "installer.exe",
        "setup.exe",
        "steamservice.exe",
        "updater.exe",
        "unitycrashhandler32.exe",
        "unitycrashhandler64.exe",
        "unins000.exe",
        "vc_redist.x64.exe",
        "easyanticheat_launcher.exe",
    ];
    for (offset, helper) in helper_names.iter().enumerate() {
        process(
            &config,
            40 + offset as u32,
            &[&format!("Z:\\Real Game\\{helper}")],
        );
    }
    assert_eq!(
        game_ready(&config, 42),
        1,
        "spoofs and helpers must not trigger"
    );

    process(
        &config,
        100,
        &["Z:\\SteamLibrary\\steamapps\\common\\Real Game\\bin\\game.exe"],
    );
    assert_eq!(game_ready(&config, 42), 0, "real game argv[0] triggers");
}

#[test]
fn unavailable_manifest_or_proc_returns_two() {
    let (_temp, mut config) = fixture();
    assert_eq!(game_ready(&config, 999), 2);
    config.proc_root = Path::new("/definitely/missing/fling-proc").into();
    assert_eq!(game_ready(&config, 42), 2);
}
