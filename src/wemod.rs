//! Optional WeMod support. WeMod is a user-installed Windows app that must run
//! inside the game's own Proton prefix and container to see the game process.
//! Fling never downloads WeMod; it only launches an existing install at boot.
use crate::{config::Config, error::Error, install, steam};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

const PREFIX_APP_DIR: &str = "drive_c/users/steamuser/AppData/Local/WeMod";

fn enabled_file(config: &Config) -> PathBuf {
    config.home.join(".config/fling/wemod-appids")
}

/// Shared WeMod app directory for users who copy an unpacked `app-*` folder
/// here instead of installing WeMod into each game's prefix.
pub fn shared_dir(config: &Config) -> PathBuf {
    config.home.join(".local/share/fling/wemod")
}

pub fn enabled_appids(config: &Config) -> BTreeSet<u32> {
    fs::read_to_string(enabled_file(config))
        .map(|text| {
            text.lines()
                .filter_map(|line| line.trim().parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

pub fn enabled(config: &Config, appid: u32) -> bool {
    enabled_appids(config).contains(&appid)
}

pub fn set_enabled(config: &Config, appid: u32, enable: bool) -> Result<(), Error> {
    let mut appids = enabled_appids(config);
    if enable {
        appids.insert(appid);
    } else {
        appids.remove(&appid);
    }
    let path = enabled_file(config);
    let parent = path
        .parent()
        .ok_or_else(|| Error::Message("invalid WeMod configuration path".into()))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(
        temp,
        "# Managed by Fling. Steam app IDs that launch WeMod at boot."
    )?;
    for appid in appids {
        writeln!(temp, "{appid}")?;
    }
    temp.persist(&path)
        .map_err(|error| Error::Io(error.error))?;
    Ok(())
}

fn version_of(dir_name: &str) -> Option<Vec<u32>> {
    dir_name
        .strip_prefix("app-")?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}

/// Finds the newest `app-<version>/WeMod.exe` below a Squirrel install root.
/// The versioned executable is used rather than the root stub, because the
/// stub exits immediately after spawning the real app.
pub fn newest_app_exe(root: &Path) -> Option<PathBuf> {
    fs::read_dir(root)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let version = version_of(&entry.file_name().to_string_lossy())?;
            let exe = entry.path().join("WeMod.exe");
            let dir = fs::symlink_metadata(entry.path()).ok()?;
            let file = fs::symlink_metadata(&exe).ok()?;
            (dir.is_dir() && !dir.file_type().is_symlink() && file.is_file())
                .then_some((version, exe))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, exe)| exe)
}

/// Candidate Proton prefixes for a game: its own library first, then the
/// default Steam root.
pub fn prefixes(config: &Config, appid: u32) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(game) = steam::game(config, appid) {
        roots.push(PathBuf::from(game.library_path));
    }
    roots.push(config.steam_root.clone());
    let mut result: Vec<PathBuf> = Vec::new();
    for root in roots {
        let prefix = root.join(format!("steamapps/compatdata/{appid}/pfx"));
        if prefix.is_dir() && !result.contains(&prefix) {
            result.push(prefix);
        }
    }
    result
}

pub fn find_exe(config: &Config, appid: u32) -> Option<PathBuf> {
    prefixes(config, appid)
        .iter()
        .find_map(|prefix| newest_app_exe(&prefix.join(PREFIX_APP_DIR)))
        .or_else(|| newest_app_exe(&shared_dir(config)))
}

/// True when WeMod is enabled for the game and an executable is available.
pub fn launchable(config: &Config, appid: u32) -> bool {
    enabled(config, appid) && find_exe(config, appid).is_some()
}

/// Detects a WeMod process that belongs to this game, either through the
/// Proton app ID in its environment or through its prefix path.
pub fn running(config: &Config, appid: u32) -> bool {
    let Ok(entries) = fs::read_dir(&config.proc_root) else {
        return false;
    };
    let marker = format!("STEAM_COMPAT_APP_ID={appid}");
    let prefix_marker = format!("/compatdata/{appid}/");
    entries.flatten().any(|entry| {
        if !entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            return false;
        }
        let Ok(cmd) = fs::read(entry.path().join("cmdline")) else {
            return false;
        };
        let cmd = String::from_utf8_lossy(&cmd)
            .replace('\\', "/")
            .to_lowercase();
        if !cmd.contains("wemod.exe") {
            return false;
        }
        cmd.contains(&prefix_marker)
            || fs::read(entry.path().join("environ"))
                .is_ok_and(|env| env.split(|b| *b == 0).any(|v| v == marker.as_bytes()))
    })
}

fn describe(config: &Config, appid: u32) -> String {
    match find_exe(config, appid) {
        Some(exe) => format!("ready ({})", exe.display()),
        None => "WeMod not found — run: fling wemod setup <appid> <WeMod-Setup.exe>".into(),
    }
}

pub fn enable(config: &Config, query: &str) -> Result<(), Error> {
    let game = install::resolve(config, query)?;
    set_enabled(config, game.appid, true)?;
    println!(
        ">>> WeMod enabled for {} (appid {}) — it will launch instead of the FLiNG trainer",
        game.name, game.appid
    );
    println!(">>> {}", describe(config, game.appid));
    Ok(())
}

pub fn disable(config: &Config, query: &str) -> Result<(), Error> {
    let game = install::resolve(config, query)?;
    set_enabled(config, game.appid, false)?;
    println!(
        ">>> WeMod disabled for {} (appid {})",
        game.name, game.appid
    );
    Ok(())
}

pub fn status(config: &Config) {
    let appids = enabled_appids(config);
    if appids.is_empty() {
        println!("(no games use WeMod)");
        return;
    }
    for appid in appids {
        let name = steam::game(config, appid)
            .map(|game| game.name)
            .unwrap_or_else(|| "(not installed)".into());
        println!("{appid}\t{name}\t{}", describe(config, appid));
    }
}

/// Runs the user-supplied WeMod installer inside the game's Proton prefix and
/// enables WeMod for that game.
pub fn setup(config: &Config, query: &str, installer: &Path, dotnet: bool) -> Result<(), Error> {
    let game = install::resolve(config, query)?;
    let meta = fs::metadata(installer).map_err(|_| {
        Error::Message(format!(
            "WeMod installer not found: {}",
            installer.display()
        ))
    })?;
    if !meta.is_file() {
        return Err(Error::Message(format!(
            "WeMod installer is not a file: {}",
            installer.display()
        )));
    }
    if prefixes(config, game.appid).is_empty() {
        return Err(Error::Message(format!(
            "no Proton prefix for {} — launch the game once first",
            game.name
        )));
    }
    let appid = game.appid.to_string();
    if dotnet {
        println!(
            ">>> Installing .NET Framework 4.8 into the game's prefix (this can take a while)..."
        );
        let status = Command::new("protontricks")
            .args([appid.as_str(), "-q", "dotnet48"])
            .status()
            .map_err(|_| Error::DependencyMissing("protontricks".into()))?;
        if !status.success() {
            return Err(Error::Message("protontricks dotnet48 failed".into()));
        }
    }
    println!(
        ">>> Running the WeMod installer in the Proton prefix of {}...",
        game.name
    );
    println!(
        ">>> Sign in when WeMod opens, then close it. Fling starts it with the game from now on."
    );
    let status = Command::new("protontricks-launch")
        .args(["--appid", appid.as_str()])
        .arg(installer)
        .status()
        .map_err(|_| Error::DependencyMissing("protontricks-launch".into()))?;
    if !status.success() {
        return Err(Error::Message("WeMod installer failed".into()));
    }
    set_enabled(config, game.appid, true)?;
    println!(
        ">>> WeMod enabled for {} — {}",
        game.name,
        describe(config, game.appid)
    );
    Ok(())
}
