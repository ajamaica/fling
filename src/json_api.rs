use crate::{config::Config, error::json_failure, process, steam};
use serde::Serialize;
use std::{fs, io::Read, os::unix::fs::OpenOptionsExt, path::PathBuf, process::Command};

pub fn steam_environment_active(config: &Config, pid: u32) -> bool {
    let path = config.proc_root.join(pid.to_string()).join("environ");
    let Ok(mut file) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
    else {
        return false;
    };
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return false;
    }
    let mut environment = Vec::new();
    file.read_to_end(&mut environment).is_ok()
        && environment
            .split(|byte| *byte == 0)
            .any(|entry| entry == b"STEAM_COMPAT_LAUNCHER_SERVICE=proton")
}

pub fn steam_environment_active_for_pids(config: &Config, pids: &[u8]) -> bool {
    String::from_utf8_lossy(pids)
        .lines()
        .filter_map(|pid| pid.parse().ok())
        .any(|pid| steam_environment_active(config, pid))
}

#[derive(Serialize)]
struct Games {
    schema_version: u8,
    games: Vec<steam::Game>,
}
#[derive(Serialize)]
struct Status {
    schema_version: u8,
    cli_installed: bool,
    watcher_installed: bool,
    watcher_active: bool,
    global_environment_configured: bool,
    steam_environment_active: bool,
    steam_running: bool,
    steam_root: String,
    trainers_directory: String,
}
#[derive(Serialize)]
struct Refresh {
    schema_version: u8,
    success: bool,
    operation: &'static str,
    appid: u32,
    name: String,
    game: steam::Game,
    message: &'static str,
}

fn print_json<T: Serialize>(v: &T) {
    match serde_json::to_string(v) {
        Ok(value) => println!("{value}"),
        Err(error) => {
            eprintln!("ERROR: JSON serialization failed: {error}");
            std::process::exit(1);
        }
    }
}
pub fn games(config: &Config, installed: bool) {
    let mut list = steam::games(config);
    if installed {
        list.retain(|g| g.trainer_installed)
    }
    print_json(&Games {
        schema_version: 1,
        games: list,
    })
}
pub fn status(config: &Config, argv0: &str) {
    let env_conf = config
        .home
        .join(".config/environment.d/10-fling-trainers.conf");
    let configured = fs::read_to_string(env_conf).is_ok_and(|s| {
        s.lines()
            .any(|l| l == "STEAM_COMPAT_LAUNCHER_SERVICE=proton")
    });
    let steam_process = Command::new("pgrep").args(["-x", "steam"]).output();
    let steam_running = steam_process
        .as_ref()
        .is_ok_and(|output| output.status.success());
    let steam_environment_active = steam_process
        .ok()
        .is_some_and(|output| steam_environment_active_for_pids(config, &output.stdout));
    let unit = config.home.join(".config/systemd/user/fling-watch.service");
    print_json(&Status {
        schema_version: 1,
        cli_installed: PathBuf::from(argv0).is_file(),
        watcher_installed: unit.is_file(),
        watcher_active: process::command_ok(
            "systemctl",
            &["--user", "is-active", "--quiet", "fling-watch.service"],
        ),
        global_environment_configured: configured,
        steam_environment_active,
        steam_running,
        steam_root: config.steam_root.to_string_lossy().into(),
        trainers_directory: config.trainers.to_string_lossy().into(),
    })
}
pub fn refresh(config: &Config, arg: &str) {
    let Ok(appid) = arg.parse() else {
        json_failure("refresh", 0, 2, "invalid_args", "appid must be numeric")
    };
    let Some(game) = steam::game(config, appid) else {
        json_failure(
            "refresh",
            appid,
            3,
            "game_missing",
            "Installed Steam game not found",
        )
    };
    print_json(&Refresh {
        schema_version: 1,
        success: true,
        operation: "refresh",
        appid,
        name: game.name.clone(),
        game,
        message: "Game state refreshed",
    })
}
