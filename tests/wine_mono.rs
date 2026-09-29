use fling_cli::{
    config::Config,
    steam,
    wine_mono::{self, Outcome},
};
use std::{fs, path::Path};
use tempfile::TempDir;

fn fixture(trainer: &[u8], proton_mono: &str) -> (TempDir, Config, steam::Game) {
    let temp = tempfile::tempdir().expect("tempdir");
    let steam_root = temp.path().join("steam");
    let compatdata = steam_root.join("steamapps/compatdata/42");
    fs::create_dir_all(compatdata.join("pfx")).expect("prefix");
    let proton = temp.path().join("Proton/files");
    fs::create_dir_all(proton.join("share/fonts")).expect("fonts");
    fs::create_dir_all(proton.join(format!("share/wine/mono/wine-mono-{proton_mono}")))
        .expect("mono");
    fs::write(
        compatdata.join("config_info"),
        format!(
            "11.0-100\n{}/share/fonts/\n{}/lib/\n",
            proton.display(),
            proton.display()
        ),
    )
    .expect("config_info");
    fs::write(
        steam_root.join("steamapps/appmanifest_42.acf"),
        "\"AppState\"\n{\n \"appid\" \"42\"\n \"name\" \"Real Game\"\n \"installdir\" \"Real Game\"\n}",
    )
    .expect("manifest");
    let trainers = temp.path().join("trainers");
    fs::create_dir_all(trainers.join("42 - Real Game")).expect("trainer dir");
    fs::write(trainers.join("42 - Real Game/Trainer.exe"), trainer).expect("trainer");
    let config = Config {
        home: temp.path().join("home"),
        trainers,
        steam_root,
        proc_root: temp.path().join("proc"),
    };
    let game = steam::game(&config, 42).expect("game");
    (temp, config, game)
}

fn runtime(config: &Config) -> std::path::PathBuf {
    config
        .steam_root
        .join("steamapps/compatdata/42/pfx/drive_c/windows/mono/mono-2.0")
}

fn managed_runtime(path: &Path) {
    fs::create_dir_all(path).expect("runtime");
    fs::write(path.join(".fling-wine-mono"), "11.3.0\n").expect("marker");
}

#[test]
fn only_clr_hosting_trainers_are_detected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let clr = temp.path().join("clr.exe");
    let native = temp.path().join("native.exe");
    fs::write(&clr, b"MZ..mscoree.dll\0CLRCreateInstance\0").expect("clr");
    fs::write(&native, b"MZ..kernel32.dll\0").expect("native");
    assert!(wine_mono::trainer_hosts_clr(&clr));
    assert!(!wine_mono::trainer_hosts_clr(&native));
}

#[test]
fn proton_mono_version_comes_from_config_info() {
    let (_temp, config, _game) = fixture(b"CLRCreateInstance", "11.2.0");
    let compatdata = config.steam_root.join("steamapps/compatdata/42");
    assert_eq!(wine_mono::proton_mono_versions(&compatdata), ["11.2.0"]);
    fs::remove_file(compatdata.join("config_info")).expect("remove");
    assert!(wine_mono::proton_mono_versions(&compatdata).is_empty());
}

#[test]
fn working_proton_mono_needs_no_override() {
    let (_temp, config, game) = fixture(b"CLRCreateInstance", "11.3.0");
    assert_eq!(
        wine_mono::ensure(&config, &game).unwrap(),
        Outcome::NotAffected
    );
    assert!(!runtime(&config).exists());
}

#[test]
fn native_trainer_on_broken_proton_needs_no_override() {
    let (_temp, config, game) = fixture(b"native only", "11.2.0");
    assert_eq!(
        wine_mono::ensure(&config, &game).unwrap(),
        Outcome::NotAffected
    );
}

#[test]
fn managed_override_is_kept_while_needed_and_removed_after() {
    let (_temp, config, game) = fixture(b"CLRCreateInstance", "11.2.0");
    managed_runtime(&runtime(&config));
    assert_eq!(
        wine_mono::ensure(&config, &game).unwrap(),
        Outcome::AlreadyInstalled
    );
    let (_temp, config, game) = fixture(b"CLRCreateInstance", "11.3.0");
    managed_runtime(&runtime(&config));
    assert_eq!(wine_mono::ensure(&config, &game).unwrap(), Outcome::Removed);
    assert!(!runtime(&config).exists());
}

#[test]
fn foreign_runtime_is_never_touched() {
    let (_temp, config, game) = fixture(b"CLRCreateInstance", "11.2.0");
    fs::create_dir_all(runtime(&config)).expect("foreign runtime");
    assert_eq!(wine_mono::ensure(&config, &game).unwrap(), Outcome::Foreign);
    let (_temp, config, game) = fixture(b"CLRCreateInstance", "11.3.0");
    fs::create_dir_all(runtime(&config)).expect("foreign runtime");
    assert_eq!(
        wine_mono::ensure(&config, &game).unwrap(),
        Outcome::NotAffected
    );
    assert!(runtime(&config).is_dir());
}

#[test]
fn archive_listing_rejects_links_and_escapes() {
    let ok = "drwxr-xr-x u/g 0 2026-08-17 18:00 wine-mono-11.3.0/\n\
              -rw-r--r-- u/g 9 2026-08-17 18:00 wine-mono-11.3.0/lib/mono/4.5/mscorlib.dll\n";
    assert!(wine_mono::listing_is_safe(ok));
    assert!(!wine_mono::listing_is_safe(
        "lrwxrwxrwx u/g 0 2026-08-17 18:00 wine-mono-11.3.0/x -> /etc/passwd\n"
    ));
    assert!(!wine_mono::listing_is_safe(
        "-rw-r--r-- u/g 9 2026-08-17 18:00 wine-mono-11.3.0/../../evil\n"
    ));
    assert!(!wine_mono::listing_is_safe(
        "-rw-r--r-- u/g 9 2026-08-17 18:00 other/file\n"
    ));
}
