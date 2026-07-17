use fling_cli::{config::Config, steam};
use std::{fs, os::unix::fs::symlink};

#[test]
fn game_directory_rejects_symlinked_ancestor_component() {
    let temp = tempfile::tempdir().expect("tempdir");
    let steam_root = temp.path().join("steam");
    let common = steam_root.join("steamapps/common");
    let redirected = common.join("RealTarget");
    fs::create_dir_all(redirected.join("Nested/Game")).expect("redirected game");
    symlink(&redirected, common.join("Linked")).expect("ancestor symlink");
    fs::write(
        steam_root.join("steamapps/appmanifest_42.acf"),
        r#""appid" "42"
"name" "Unsafe Game"
"installdir" "Linked/Nested/Game"
"#,
    )
    .expect("manifest");
    let config = Config {
        home: temp.path().join("home"),
        trainers: temp.path().join("trainers"),
        steam_root,
        proc_root: temp.path().join("proc"),
    };

    assert!(
        steam::game_dir(&config, 42).is_err(),
        "every manifest install-dir component must be checked without following symlinks"
    );
}

#[test]
fn game_directory_accepts_valid_nested_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let steam_root = temp.path().join("steam");
    let game = steam_root.join("steamapps/common/Publisher/Game");
    fs::create_dir_all(&game).expect("nested game");
    fs::write(
        steam_root.join("steamapps/appmanifest_42.acf"),
        r#""appid" "42" "name" "Safe Game" "installdir" "Publisher/Game""#,
    )
    .expect("manifest");
    let config = Config {
        home: temp.path().join("home"),
        trainers: temp.path().join("trainers"),
        steam_root,
        proc_root: temp.path().join("proc"),
    };

    let directory = steam::game_dir(&config, 42).expect("safe game");
    let written = directory
        .write_exclusive(".descriptor-test-", b"nested")
        .expect("write through verified descriptor");
    assert_eq!(
        fs::read(game.join(written)).expect("written file"),
        b"nested"
    );
}

#[test]
fn opened_game_directory_survives_path_replacement_without_redirection() {
    let temp = tempfile::tempdir().expect("tempdir");
    let steam_root = temp.path().join("steam");
    let game = steam_root.join("steamapps/common/Game");
    let attacker = steam_root.join("steamapps/common/Attacker");
    fs::create_dir_all(&game).expect("game");
    fs::create_dir(&attacker).expect("attacker");
    fs::write(
        steam_root.join("steamapps/appmanifest_42.acf"),
        r#""appid" "42" "name" "Safe Game" "installdir" "Game""#,
    )
    .expect("manifest");
    let config = Config {
        home: temp.path().join("home"),
        trainers: temp.path().join("trainers"),
        steam_root,
        proc_root: temp.path().join("proc"),
    };
    let directory = steam::game_dir(&config, 42).expect("verified game descriptor");
    let moved = game.with_file_name("OriginalGame");
    fs::rename(&game, &moved).expect("rename original game");
    symlink(&attacker, &game).expect("replace pathname with symlink");

    let written = directory
        .write_exclusive(".descriptor-test-", b"original")
        .expect("write through retained descriptor");

    assert_eq!(
        fs::read(moved.join(written)).expect("original file"),
        b"original"
    );
    assert!(
        fs::read_dir(attacker)
            .expect("attacker directory")
            .next()
            .is_none()
    );
}
