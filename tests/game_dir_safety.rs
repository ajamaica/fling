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
