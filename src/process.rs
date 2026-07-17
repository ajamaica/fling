use crate::{config::Config, steam};
use std::{fs, path::Path, process::Command};

pub fn command_ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

pub fn game_ready(config: &Config, appid: u32) -> i32 {
    let Some(game) = steam::game(config, appid) else {
        return 2;
    };
    if game.install_dir.is_empty()
        || Path::new(&game.install_dir).is_absolute()
        || Path::new(&game.install_dir)
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return 2;
    }
    let needle = format!(
        "/{}/",
        game.install_dir
            .replace('\\', "/")
            .trim_matches('/')
            .to_lowercase()
    );
    let Ok(entries) = fs::read_dir(&config.proc_root) else {
        return 2;
    };
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        let Ok(env) = fs::read(entry.path().join("environ")) else {
            continue;
        };
        let marker = format!("STEAM_COMPAT_APP_ID={appid}");
        if !env.split(|b| *b == 0).any(|v| v == marker.as_bytes()) {
            continue;
        }
        let Ok(cmd) = fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let argv0 = cmd.split(|byte| *byte == 0).next().unwrap_or_default();
        let normalized = String::from_utf8_lossy(argv0)
            .replace('\\', "/")
            .to_lowercase();
        let helper_dirs = [
            "/_commonredist/",
            "/redist/",
            "/redistributable/",
            "/__installer/",
            "/installer/",
            "/installers/",
            "/prerequisites/",
            "/prereqs/",
            "/supportsoftware/",
            "/easyanticheat/",
            "/anticheat/",
        ];
        let name = normalized.rsplit('/').next().unwrap_or_default();
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
            "eadesktop.exe",
            "eaappinstaller.exe",
        ];
        if helper_dirs.iter().any(|marker| normalized.contains(marker))
            || helper_names.contains(&name)
            || name.starts_with("unins")
            || name.starts_with("vc_redist")
            || name.starts_with("easyanticheat")
        {
            continue;
        }
        if normalized.contains(&needle) && normalized.contains(".exe") {
            return 0;
        }
    }
    1
}
