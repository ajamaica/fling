use std::process::Command;

#[test]
fn home_absence_never_creates_relative_config_paths() {
    let cwd = tempfile::tempdir().expect("temporary working directory");
    let output = Command::new(env!("CARGO_BIN_EXE_fling-rs"))
        .arg("_steamroot")
        .env_remove("HOME")
        .env_remove("FLING_STEAM_ROOT")
        .current_dir(cwd.path())
        .output()
        .expect("run fling");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let steam_root = String::from_utf8(output.stdout).expect("UTF-8 path");
    let steam_root = std::path::Path::new(steam_root.trim());
    assert!(
        steam_root.is_absolute(),
        "steam root must not be relative: {steam_root:?}"
    );
    assert!(
        !steam_root.starts_with(cwd.path()),
        "steam root must not be derived from cwd"
    );
}
