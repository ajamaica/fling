use crate::{config::Config, error::Error, install, process::command_ok, steam};
use std::{env, fs, process::Command, thread, time::Duration};

fn steam_launch_appid(command: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(command).replace('\0', " ");
    if !text.contains("reaper SteamLaunch") {
        return None;
    }
    let rest = text.split_once("AppId=")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

fn launch_ancestor_appid(config: &Config) -> Option<String> {
    let mut pid = std::process::id();
    for _ in 0..10 {
        let stat = fs::read_to_string(config.proc_root.join(pid.to_string()).join("stat")).ok()?;
        let suffix = stat.rsplit_once(')')?.1;
        pid = suffix.split_whitespace().nth(1)?.parse().ok()?;
        if pid <= 1 {
            return None;
        }
        let command = fs::read(config.proc_root.join(pid.to_string()).join("cmdline")).ok()?;
        if let Some(appid) = steam_launch_appid(&command) {
            return Some(appid);
        }
    }
    None
}

fn start_window_tagger(config: &Config) {
    let Some(appid) = launch_ancestor_appid(config) else {
        return;
    };
    thread::spawn(move || {
        for _ in 0..40 {
            let sockets = fs::read_dir("/tmp/.X11-unix").ok();
            for socket in sockets.into_iter().flatten().flatten() {
                let name = socket.file_name();
                let Some(display) = name.to_str().and_then(|name| name.strip_prefix('X')) else {
                    continue;
                };
                let display = format!(":{display}");
                let output = Command::new("timeout")
                    .args(["3", "xdotool", "search", "--name", "-i", "trainer"])
                    .env("DISPLAY", &display)
                    .output();
                let Some(window) = output
                    .ok()
                    .filter(|result| result.status.success())
                    .and_then(|result| {
                        String::from_utf8_lossy(&result.stdout)
                            .lines()
                            .next()
                            .map(str::to_owned)
                    })
                else {
                    continue;
                };
                if Command::new("xprop")
                    .args([
                        "-id",
                        &window,
                        "-f",
                        "STEAM_GAME",
                        "32c",
                        "-set",
                        "STEAM_GAME",
                        &appid,
                    ])
                    .env("DISPLAY", &display)
                    .status()
                    .is_ok_and(|status| status.success())
                {
                    return;
                }
            }
            thread::sleep(Duration::from_secs(3));
        }
    });
}
pub fn list(config: &Config) {
    for g in steam::games(config) {
        println!("{}\t{}", g.appid, g.name)
    }
}
pub fn installed(config: &Config) {
    let mut names = Vec::new();
    if let Ok(es) = fs::read_dir(&config.trainers) {
        for e in es.flatten() {
            if e.path().is_dir() {
                names.push(e.file_name().to_string_lossy().into_owned())
            }
        }
    }
    names.sort();
    if names.is_empty() {
        println!("(none)")
    } else {
        for n in names {
            println!("{n}")
        }
    }
}
pub fn setup(config: &Config, q: Option<&str>) -> Result<(), Error> {
    if let Some(q) = q {
        let g = install::resolve(config, q)?;
        if steam::find_trainer(config, g.appid).is_none() {
            println!(
                ">>> note: no trainer downloaded yet for {} — run: fling get {}",
                g.name, g.appid
            )
        }
        println!(
            ">>> Configuring trainer injection (global, applies to {} and all games)...",
            g.name
        )
    } else {
        println!(">>> Configuring trainer injection globally for all games...")
    }
    let p = config
        .home
        .join(".config/environment.d/10-fling-trainers.conf");
    let parent = p
        .parent()
        .ok_or_else(|| Error::Message("invalid environment configuration path".into()))?;
    fs::create_dir_all(parent)?;
    if !fs::read_to_string(&p).is_ok_and(|s| {
        s.lines()
            .any(|l| l == "STEAM_COMPAT_LAUNCHER_SERVICE=proton")
    }) {
        fs::write(
            &p,
            "# Managed by Fling. Make every Proton game expose its launcher service.\nSTEAM_COMPAT_LAUNCHER_SERVICE=proton\n",
        )?;
        println!(">>> Wrote {}", p.display())
    } else {
        println!(">>> {} already present ✓", p.display())
    }
    let _ = Command::new("systemctl")
        .args([
            "--user",
            "set-environment",
            "STEAM_COMPAT_LAUNCHER_SERVICE=proton",
        ])
        .status();
    Ok(())
}
pub fn run(config: &Config, q: &str) -> Result<(), Error> {
    let g = install::resolve(config, q)?;
    let exe = steam::find_trainer(config, g.appid).ok_or_else(|| {
        Error::Message(format!(
            "no trainer installed for '{}' — run: fling get {}",
            g.name, g.appid
        ))
    })?;
    if !config
        .steam_root
        .join(format!("steamapps/compatdata/{}/pfx", g.appid))
        .is_dir()
    {
        return Err(Error::Message(format!(
            "no Proton prefix for {} — launch the game once first",
            g.name
        )));
    }
    println!(
        ">>> Launching trainer for {} (appid {}) in its Proton prefix...",
        g.name, g.appid
    );
    let session = if env::var_os("DISPLAY").is_none() && env::var_os("WAYLAND_DISPLAY").is_none() {
        crate::watcher::steam_session_environment(config)
    } else {
        Default::default()
    };
    start_window_tagger(config);
    let launch_client = fs::read_dir(config.steam_root.join("steamapps/common"))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("SteamLinuxRuntime_")
        })
        .map(|entry| {
            entry
                .path()
                .join("pressure-vessel/bin/steam-runtime-launch-client")
        })
        .find(|path| path.is_file());
    let service_active = Command::new("busctl")
        .args(["--user", "list", "--no-legend"])
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && crate::watcher::parse_service_instances(&String::from_utf8_lossy(&output.stdout))
                    .iter()
                    .any(|service| service.appid == g.appid)
        });
    let status = if let Some(client) = launch_client.filter(|_| service_active) {
        println!(">>> Game launcher service detected — injecting into the game's container ✓");
        Command::new(client)
            .arg(format!("--bus-name=com.steampowered.App{}", g.appid))
            .arg("--directory")
            .arg(&config.home)
            .args(["--", "wine"])
            .arg(exe)
            .envs(&session)
            .status()?
    } else {
        println!(">>> WARNING: game launcher service not found (is the game running,");
        println!(">>> with launch options STEAM_COMPAT_LAUNCHER_SERVICE=proton %command% ?)");
        println!(">>> Falling back to a separate container — trainer will NOT see the game.");
        Command::new("protontricks-launch")
            .args(["--appid", &g.appid.to_string()])
            .arg(exe)
            .envs(&session)
            .status()?
    };
    if status.success() {
        Ok(())
    } else {
        Err(Error::Message("trainer launch failed".into()))
    }
}
pub fn restart() -> Result<(), Error> {
    if command_ok("pgrep", &["-f", "reaper SteamLaunch"]) {
        return Err(Error::Message(
            "a game is running — close it first, then: fling restart-steam".into(),
        ));
    }
    let _ = Command::new("systemctl")
        .args([
            "--user",
            "set-environment",
            "STEAM_COMPAT_LAUNCHER_SERVICE=proton",
        ])
        .status();
    let unit = Command::new("systemctl")
        .args([
            "--user",
            "list-units",
            "--no-legend",
            "gamescope-session-plus@*",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "gamescope-session-plus@steam.service".into());
    let status = Command::new("systemctl")
        .args(["--user", "restart", &unit])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Message(
            "could not restart gamescope session — reboot to activate instead".into(),
        ))
    }
}

pub fn lo_edit(path: &str, appid: &str, mode: &str) -> Result<i32, Error> {
    let mut text = fs::read_to_string(path)?;
    let marker = format!("\t\t\t\t\t\"{appid}\"\n\t\t\t\t\t{{\n");
    let want = "STEAM_COMPAT_LAUNCHER_SERVICE=proton";
    if let Some(start) = text.find(&marker) {
        let body_start = start + marker.len();
        let Some(relative_end) = text[body_start..].find("\n\t\t\t\t\t}") else {
            return Ok(3);
        };
        let end = body_start + relative_end;
        if text[body_start..end].contains(want) {
            println!("already configured");
            return Ok(2);
        }
        if mode == "check" {
            println!("needs edit");
            return Ok(0);
        }
        let body = &text[body_start..end];
        if let Some(key) = body.find("\"LaunchOptions\"") {
            let after_key = body_start + key + "\"LaunchOptions\"".len();
            let Some(open_relative) = text[after_key..end].find('"') else {
                return Ok(3);
            };
            let value_start = after_key + open_relative + 1;
            let mut escaped = false;
            let mut value_end = None;
            for (offset, character) in text[value_start..end].char_indices() {
                if character == '"' && !escaped {
                    value_end = Some(value_start + offset);
                    break;
                }
                escaped = character == '\\' && !escaped;
                if character != '\\' {
                    escaped = false;
                }
            }
            let Some(value_end) = value_end else {
                return Ok(3);
            };
            let current = &text[value_start..value_end];
            let replacement = if current.contains("%command%") {
                format!("{want} {current}")
            } else if current.trim().is_empty() {
                format!("{want} %command%")
            } else {
                format!("{want} %command% {current}")
            };
            text.replace_range(value_start..value_end, &replacement);
        } else {
            text.insert_str(
                body_start,
                &format!("\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{want} %command%\"\n"),
            );
        }
    } else {
        if mode == "check" {
            println!("needs edit (new app block)");
            return Ok(0);
        }
        let apps = "\t\t\t\t\"apps\"\n\t\t\t\t{\n";
        let Some(position) = text.find(apps).map(|p| p + apps.len()) else {
            println!("apps section not found in {path}");
            return Ok(3);
        };
        let block = format!(
            "{marker}\t\t\t\t\t\t\"LaunchOptions\"\t\t\"{want} %command%\"\n\t\t\t\t\t}}\n"
        );
        text.insert_str(position, &block);
    }
    if mode == "apply" {
        fs::write(path, text)?;
        println!("edited")
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_edit_preserves_existing_launch_options() {
        let temp = tempfile::NamedTempFile::new().expect("tempfile");
        let content = "\t\t\t\t\"apps\"\n\t\t\t\t{\n\t\t\t\t\t\"42\"\n\t\t\t\t\t{\n\t\t\t\t\t\t\"LaunchOptions\"\t\t\"DXVK_ASYNC=1 %command% -windowed\"\n\t\t\t\t\t}\n\t\t\t\t}\n";
        fs::write(temp.path(), content).expect("fixture");
        assert_eq!(
            lo_edit(temp.path().to_str().expect("path"), "42", "apply").expect("edit"),
            0
        );
        let edited = fs::read_to_string(temp.path()).expect("edited");
        assert!(edited.contains(
            "\"LaunchOptions\"\t\t\"STEAM_COMPAT_LAUNCHER_SERVICE=proton DXVK_ASYNC=1 %command% -windowed\""
        ));
        assert_eq!(edited.matches("LaunchOptions").count(), 1);
    }

    #[test]
    fn extracts_only_numeric_steam_launch_appid() {
        assert_eq!(
            steam_launch_appid(b"reaper SteamLaunch AppId=12345 -- foo"),
            Some("12345".into())
        );
        assert_eq!(steam_launch_appid(b"reaper SteamLaunch AppId=bad"), None);
        assert_eq!(steam_launch_appid(b"AppId=12345"), None);
    }
}
