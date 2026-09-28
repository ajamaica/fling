//! Optional WeMod support. WeMod is a Windows app that must run inside the
//! game's own Proton prefix and container to see the game process. One install
//! and one signed-in profile are shared by every game.
use crate::{
    config::Config,
    error::{Error, json_failure},
    install, steam,
};
use serde::Serialize;
use sha2::Digest;
use std::sync::atomic::{AtomicBool, Ordering};

/// In JSON mode stdout carries only the JSON result, so progress text and
/// installer output go to stderr instead.
static JSON_MODE: AtomicBool = AtomicBool::new(false);

macro_rules! say {
    ($($arg:tt)*) => {
        if JSON_MODE.load(Ordering::Relaxed) {
            eprintln!($($arg)*)
        } else {
            println!($($arg)*)
        }
    };
}

fn child_stdout() -> std::process::Stdio {
    if JSON_MODE.load(Ordering::Relaxed) {
        std::io::stderr().into()
    } else {
        std::process::Stdio::inherit()
    }
}
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

/// WeMod's official installer download (the site's "Download" button).
/// Override with FLING_WEMOD_URL if it moves.
const DOWNLOAD_URL: &str = "https://api.wemod.com/client/download";
const DOWNLOAD_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Firefox/128.0";

const PREFIX_APP_DIR: &str = "drive_c/users/steamuser/AppData/Local/WeMod";
const PREFIX_ROAMING_DIR: &str = "drive_c/users/steamuser/AppData/Roaming";

fn enabled_file(config: &Config) -> PathBuf {
    config.home.join(".config/fling/wemod-appids")
}

/// Shared WeMod install (the Squirrel root with `app-*` and `Update.exe`),
/// used by every game so WeMod is installed and updated once.
pub fn shared_dir(config: &Config) -> PathBuf {
    config.home.join(".local/share/fling/wemod")
}

/// Shared WeMod profile (`%APPDATA%\\WeMod`) that holds the signed-in
/// session. Each game's prefix links to it, so signing in once is enough.
pub fn profile_dir(config: &Config) -> PathBuf {
    config.home.join(".local/share/fling/wemod-profile")
}

/// Which trainer(s) start with a game. All three are equal options chosen
/// per game; games without a saved choice keep using FLiNG.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Choice {
    Fling,
    Wemod,
    Both,
}

impl Choice {
    pub fn as_str(self) -> &'static str {
        match self {
            Choice::Fling => "fling",
            Choice::Wemod => "wemod",
            Choice::Both => "both",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "fling" => Some(Choice::Fling),
            "wemod" => Some(Choice::Wemod),
            "both" => Some(Choice::Both),
            _ => None,
        }
    }

    pub fn uses_fling(self) -> bool {
        self != Choice::Wemod
    }

    pub fn uses_wemod(self) -> bool {
        self != Choice::Fling
    }
}

/// Saved per-game choices: one `<appid> wemod|both` per line. FLiNG is not
/// stored because it is what every other game uses.
pub fn choices(config: &Config) -> BTreeMap<u32, Choice> {
    fs::read_to_string(enabled_file(config))
        .map(|text| {
            text.lines()
                .filter_map(|line| {
                    let mut fields = line.split_whitespace();
                    let appid = fields.next()?.parse().ok()?;
                    let choice = Choice::parse(fields.next()?)?;
                    choice.uses_wemod().then_some((appid, choice))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Games whose choice includes WeMod.
pub fn enabled_appids(config: &Config) -> BTreeSet<u32> {
    choices(config).into_keys().collect()
}

pub fn choice(config: &Config, appid: u32) -> Choice {
    choices(config)
        .get(&appid)
        .copied()
        .unwrap_or(Choice::Fling)
}

pub fn set_choice(config: &Config, appid: u32, choice: Choice) -> Result<(), Error> {
    let mut choices = choices(config);
    if choice.uses_wemod() {
        choices.insert(appid, choice);
    } else {
        choices.remove(&appid);
    }
    let path = enabled_file(config);
    let parent = path
        .parent()
        .ok_or_else(|| Error::Message("invalid trainer choice configuration path".into()))?;
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    writeln!(
        temp,
        "# Managed by Fling. Per-game trainer choice (wemod or both); other games use FLiNG."
    )?;
    for (appid, choice) in choices {
        writeln!(temp, "{appid} {}", choice.as_str())?;
    }
    temp.persist(&path)
        .map_err(|error| Error::Io(error.error))?;
    Ok(())
}

/// What `fling run` / the watcher start for a game.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Plan {
    pub trainer: Option<PathBuf>,
    pub wemod: Option<PathBuf>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.trainer.is_none() && self.wemod.is_none()
    }
}

pub fn plan(config: &Config, appid: u32) -> Plan {
    let choice = choice(config, appid);
    Plan {
        trainer: choice
            .uses_fling()
            .then(|| steam::find_trainer(config, appid))
            .flatten(),
        wemod: choice
            .uses_wemod()
            .then(|| find_exe(config, appid))
            .flatten(),
    }
}

fn version_of(dir_name: &str) -> Option<Vec<u32>> {
    dir_name
        .strip_prefix("app-")?
        .split('.')
        .map(|part| part.parse().ok())
        .collect()
}

fn newest_app(root: &Path) -> Option<(Vec<u32>, PathBuf)> {
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
}

/// Finds the newest `app-<version>/WeMod.exe` below a Squirrel install root.
/// The versioned executable is used rather than the root stub, because the
/// stub exits immediately after spawning the real app.
pub fn newest_app_exe(root: &Path) -> Option<PathBuf> {
    newest_app(root).map(|(_, exe)| exe)
}

/// Candidate Proton prefixes for a game: its own library first, then the
/// default Steam root.
pub fn prefixes(config: &Config, appid: u32) -> Vec<PathBuf> {
    let library = steam::game(config, appid).map(|game| PathBuf::from(game.library_path));
    prefixes_in(config, appid, library.as_deref())
}

fn prefixes_in(config: &Config, appid: u32, library: Option<&Path>) -> Vec<PathBuf> {
    let roots = library.into_iter().chain([config.steam_root.as_path()]);
    let mut result: Vec<PathBuf> = Vec::new();
    for root in roots {
        let prefix = root.join(format!("steamapps/compatdata/{appid}/pfx"));
        if prefix.is_dir() && !result.contains(&prefix) {
            result.push(prefix);
        }
    }
    result
}

/// The newest WeMod available to a game: its own prefix install or the shared
/// one. On equal versions the shared install wins, so updates are shared too.
pub fn find_exe(config: &Config, appid: u32) -> Option<PathBuf> {
    newest_for(config, &prefixes(config, appid))
}

/// Like `find_exe` for a game whose library is already known (used while
/// listing games, which `prefixes` itself depends on).
pub fn find_exe_in_library(config: &Config, appid: u32, library: &Path) -> Option<PathBuf> {
    newest_for(config, &prefixes_in(config, appid, Some(library)))
}

fn newest_for(config: &Config, prefixes: &[PathBuf]) -> Option<PathBuf> {
    prefixes
        .iter()
        .map(|prefix| prefix.join(PREFIX_APP_DIR))
        .chain([shared_dir(config)])
        .filter_map(|root| newest_app(&root))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, exe)| exe)
}

/// Copies `from` into `to` without following symlinks.
fn copy_tree(from: &Path, to: &Path) -> Result<(), Error> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Copies a WeMod install from the game's prefix into the shared install,
/// adding only entries (such as a new `app-*`) the shared install lacks.
pub fn import_install(config: &Config, appid: u32) -> Result<bool, Error> {
    let shared = shared_dir(config);
    let mut imported = false;
    for prefix in prefixes(config, appid) {
        let root = prefix.join(PREFIX_APP_DIR);
        if fs::symlink_metadata(&root).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let target = shared.join(entry.file_name());
            if fs::symlink_metadata(&target).is_ok() {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            fs::create_dir_all(&shared)?;
            if kind.is_dir() {
                // Copy under a temporary name so an interrupted copy is never
                // mistaken for a complete app directory.
                let partial = shared.join(format!(
                    ".partial-{}-{}",
                    std::process::id(),
                    entry.file_name().to_string_lossy()
                ));
                let _ = fs::remove_dir_all(&partial);
                copy_tree(&entry.path(), &partial)?;
                fs::rename(&partial, &target)?;
            } else if kind.is_file() {
                fs::copy(entry.path(), &target)?;
            } else {
                continue;
            }
            imported = true;
        }
    }
    Ok(imported)
}

/// Returns WeMod for this game, first sharing an install found in any game's
/// prefix, so WeMod is never downloaded or installed twice.
pub fn find_or_import(config: &Config, appid: u32) -> Option<PathBuf> {
    if let Some(exe) = find_exe(config, appid) {
        return Some(exe);
    }
    for game in steam::games(config) {
        if let Err(error) = import_install(config, game.appid) {
            say!(
                ">>> WARNING: could not share {}'s WeMod install: {error}",
                game.name
            );
        }
    }
    find_exe(config, appid)
}

fn backup_path(path: &Path) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let mut candidate = path.with_file_name(format!("WeMod.fling-backup-{stamp}"));
    let mut n = 1;
    while fs::symlink_metadata(&candidate).is_ok() {
        candidate = path.with_file_name(format!("WeMod.fling-backup-{stamp}-{n}"));
        n += 1;
    }
    candidate
}

/// Points the game's `%APPDATA%\\WeMod` at the shared profile so the WeMod
/// session is shared by every game. The first game's existing profile is
/// adopted as the shared one; later games' own profiles are kept as backups,
/// never deleted.
pub fn share_profile(config: &Config, appid: u32) -> Result<(), Error> {
    let prefix = prefixes(config, appid)
        .into_iter()
        .next()
        .ok_or_else(|| Error::Message("no Proton prefix — launch the game once first".into()))?;
    let roaming = prefix.join(PREFIX_ROAMING_DIR);
    let link = roaming.join("WeMod");
    let shared = profile_dir(config);
    match fs::symlink_metadata(&link) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(&link)? == shared {
                fs::create_dir_all(&shared)?;
                return Ok(());
            }
            fs::remove_file(&link)?;
        }
        Ok(meta) => {
            if meta.is_dir() && fs::symlink_metadata(&shared).is_err() {
                let parent = shared
                    .parent()
                    .ok_or_else(|| Error::Message("invalid WeMod profile path".into()))?;
                let partial = parent.join(format!(".wemod-profile-partial-{}", std::process::id()));
                let _ = fs::remove_dir_all(&partial);
                copy_tree(&link, &partial)?;
                fs::rename(&partial, &shared)?;
                say!(
                    ">>> Using this game's WeMod sign-in for all games ({})",
                    shared.display()
                );
            }
            fs::rename(&link, backup_path(&link))?;
        }
        Err(_) => {}
    }
    fs::create_dir_all(&shared)?;
    fs::create_dir_all(&roaming)?;
    std::os::unix::fs::symlink(&shared, &link)?;
    Ok(())
}

/// Detects a WeMod process that belongs to this game, either through the
/// Proton app ID in its environment or through its prefix path.
pub fn running(config: &Config, appid: u32) -> bool {
    let Ok(entries) = fs::read_dir(&config.proc_root) else {
        return false;
    };
    let marker = format!("STEAM_COMPAT_APP_ID={appid}");
    let prefix_marker = format!("/compatdata/{appid}/");
    let wineprefix_marker = format!("/compatdata/{appid}/pfx");
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
            || fs::read(entry.path().join("environ")).is_ok_and(|env| {
                env.split(|b| *b == 0).any(|v| {
                    v == marker.as_bytes()
                        || (v.starts_with(b"WINEPREFIX=")
                            && String::from_utf8_lossy(v)
                                .trim_end_matches('/')
                                .ends_with(&wineprefix_marker))
                })
            })
    })
}

fn describe(config: &Config, appid: u32) -> String {
    match find_exe(config, appid) {
        Some(exe) => format!("ready ({})", exe.display()),
        None => format!("WeMod not installed — run: fling wemod install {appid}"),
    }
}

/// `fling use <game> [fling|wemod|both]`: shows or sets the game's choice.
pub fn choose(config: &Config, query: &str, value: Option<&str>) -> Result<(), Error> {
    let game = install::resolve(config, query)?;
    if let Some(value) = value {
        let choice = Choice::parse(value).ok_or_else(|| {
            Error::Message(format!(
                "unknown choice '{value}' — use fling, wemod or both"
            ))
        })?;
        set_choice(config, game.appid, choice)?;
    }
    let choice = choice(config, game.appid);
    say!("{}\t{}\t{}", game.appid, game.name, choice.as_str());
    if choice.uses_fling() && steam::find_trainer(config, game.appid).is_none() {
        say!(
            ">>> FLiNG trainer not installed — run: fling get {}",
            game.appid
        );
    }
    if choice.uses_wemod() && find_or_import(config, game.appid).is_none() {
        if value.is_some() && !prefixes(config, game.appid).is_empty() {
            say!(">>> WeMod is not installed yet — installing it automatically...");
            return install(config, &game.appid.to_string(), false).map_err(|error| {
                Error::Message(format!(
                    "{error} — the choice was saved; retry with: fling wemod install {}",
                    game.appid
                ))
            });
        }
        say!(
            ">>> WeMod not installed — launch the game once, then run: fling wemod install {}",
            game.appid
        );
    }
    Ok(())
}

pub fn status(config: &Config) {
    say!("install\t{}", shared_dir(config).display());
    say!("sign-in\t{}", profile_dir(config).display());
    let choices = choices(config);
    if choices.is_empty() {
        say!("(no games use WeMod)");
        return;
    }
    for (appid, choice) in choices {
        let name = steam::game(config, appid)
            .map(|game| game.name)
            .unwrap_or_else(|| "(not installed)".into());
        say!(
            "{appid}\t{name}\t{}\t{}",
            choice.as_str(),
            describe(config, appid)
        );
    }
}

/// Runs the user-supplied WeMod installer inside the game's Proton prefix.
/// It does not change which trainer the game uses; see `choose`.
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
        say!(">>> Installing .NET Framework 4.8 into the game's prefix (this can take a while)...");
        let status = Command::new("protontricks")
            .args([appid.as_str(), "-q", "dotnet48"])
            .stdout(child_stdout())
            .status()
            .map_err(|_| Error::DependencyMissing("protontricks".into()))?;
        if !status.success() {
            return Err(Error::Message("protontricks dotnet48 failed".into()));
        }
    }
    // Link the shared profile first so the sign-in below is shared by all games.
    share_profile(config, game.appid)?;
    say!(
        ">>> Running the WeMod installer in the Proton prefix of {}...",
        game.name
    );
    say!(">>> Sign in when WeMod opens, then close it.");
    let status = Command::new("protontricks-launch")
        .args(["--appid", appid.as_str()])
        .arg(installer)
        .stdout(child_stdout())
        .status()
        .map_err(|_| Error::DependencyMissing("protontricks-launch".into()))?;
    if !status.success() {
        return Err(Error::Message("WeMod installer failed".into()));
    }
    if import_install(config, game.appid)? {
        say!(
            ">>> Shared the WeMod install with all games ({})",
            shared_dir(config).display()
        );
    }
    say!(
        ">>> WeMod for {}: {}",
        game.name,
        describe(config, game.appid)
    );
    say!(">>> Other games reuse this install and sign-in — just run: fling use <game> wemod");
    say!(
        ">>> Choose what starts with the game: fling use {} fling|wemod|both (now: {})",
        game.appid,
        choice(config, game.appid).as_str()
    );
    Ok(())
}

/// Downloads the official WeMod installer and runs it through `setup`.
pub fn install(config: &Config, query: &str, dotnet: bool) -> Result<(), Error> {
    let game = install::resolve(config, query)?;
    if prefixes(config, game.appid).is_empty() {
        return Err(Error::Message(format!(
            "no Proton prefix for {} — launch the game once first",
            game.name
        )));
    }
    let cache = config.home.join(".cache/fling");
    fs::create_dir_all(&cache)?;
    let stage = tempfile::Builder::new()
        .prefix(".wemod-download-")
        .tempdir_in(&cache)?;
    let installer = stage.path().join("WeMod-Setup.exe");
    let url = std::env::var("FLING_WEMOD_URL").unwrap_or_else(|_| DOWNLOAD_URL.into());
    say!(">>> Downloading the WeMod installer from {url}...");
    let status = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "15",
            "--max-time",
            "600",
            "--max-filesize",
            "536870912",
            "-A",
            DOWNLOAD_UA,
            "-o",
        ])
        .arg(&installer)
        .arg(&url)
        .stdout(child_stdout())
        .status()
        .map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Error::DependencyMissing("curl".into()),
            _ => Error::Io(error),
        })?;
    if !status.success() {
        return Err(Error::Network("WeMod download failed".into()));
    }
    let detected = Command::new("file")
        .arg("-b")
        .arg(&installer)
        .output()
        .map_err(|_| Error::DependencyMissing("file".into()))?;
    if !String::from_utf8_lossy(&detected.stdout).contains("PE32") {
        return Err(Error::InvalidPayload(
            "the WeMod download is not a Windows installer".into(),
        ));
    }
    let bytes = fs::read(&installer)?;
    setup(config, &game.appid.to_string(), &installer, dotnet)?;
    // Diagnostic change tracking only, like trainer-metadata.json.
    let metadata = serde_json::json!({
        "schema_version": 1,
        "download_url": url,
        "sha256": format!("{:x}", sha2::Sha256::digest(&bytes)),
        "installed_at": format!(
            "{}Z",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ),
    });
    fs::create_dir_all(shared_dir(config))?;
    fs::write(
        shared_dir(config).join("fling-install.json"),
        serde_json::to_vec_pretty(&metadata)?,
    )?;
    Ok(())
}

#[derive(Serialize)]
struct JsonResult {
    schema_version: u8,
    success: bool,
    operation: &'static str,
    appid: u32,
    name: String,
    trainer_choice: &'static str,
    wemod_installed: bool,
    message: String,
}

fn json_game(config: &Config, operation: &str, arg: &str) -> steam::Game {
    JSON_MODE.store(true, Ordering::Relaxed);
    let Ok(appid) = arg.parse() else {
        json_failure(operation, 0, 2, "invalid_args", "appid must be numeric")
    };
    steam::game(config, appid).unwrap_or_else(|| {
        json_failure(
            operation,
            appid,
            3,
            "game_missing",
            "Installed Steam game not found",
        )
    })
}

fn json_success(config: &Config, operation: &'static str, game: steam::Game, message: String) {
    let result = JsonResult {
        schema_version: 1,
        success: true,
        operation,
        appid: game.appid,
        trainer_choice: choice(config, game.appid).as_str(),
        wemod_installed: find_exe(config, game.appid).is_some(),
        name: game.name,
        message,
    };
    match serde_json::to_string(&result) {
        Ok(value) => println!("{value}"),
        Err(error) => json_failure(
            operation,
            result.appid,
            1,
            "general_error",
            error.to_string(),
        ),
    }
}

/// `fling use <appid> fling|wemod|both --json`: saves the choice only; the UI
/// installs WeMod separately so it can show that step.
pub fn choose_json(config: &Config, arg: &str, value: &str) {
    let game = json_game(config, "use", arg);
    let Some(choice) = Choice::parse(value) else {
        json_failure(
            "use",
            game.appid,
            2,
            "invalid_args",
            "choice must be fling, wemod or both",
        )
    };
    if let Err(error) = set_choice(config, game.appid, choice) {
        json_failure("use", game.appid, 1, "general_error", error.to_string())
    }
    // Share an install found in another game's prefix so the UI sees it.
    if choice.uses_wemod() {
        find_or_import(config, game.appid);
    }
    let message = match choice {
        Choice::Fling => "FLiNG trainer selected",
        Choice::Wemod => "WeMod selected",
        Choice::Both => "FLiNG trainer and WeMod selected",
    };
    json_success(config, "use", game, message.into());
}

/// `fling wemod install <appid> --json`.
pub fn install_json(config: &Config, arg: &str) {
    let game = json_game(config, "wemod_install", arg);
    let appid = game.appid;
    match install(config, &appid.to_string(), false) {
        Ok(()) => json_success(
            config,
            "wemod_install",
            game,
            "WeMod installed — sign in once in the WeMod window; every game shares it".into(),
        ),
        Err(Error::Network(message)) => {
            json_failure("wemod_install", appid, 5, "network_error", message)
        }
        Err(Error::InvalidPayload(message)) => {
            json_failure("wemod_install", appid, 6, "invalid_file", message)
        }
        Err(Error::DependencyMissing(name)) => json_failure(
            "wemod_install",
            appid,
            8,
            "dependency_missing",
            format!("Missing required dependency: {name}"),
        ),
        Err(error) => json_failure(
            "wemod_install",
            appid,
            1,
            "general_error",
            error.to_string(),
        ),
    }
}
