//! Wine Mono override for trainers under Proton 11.0.
//!
//! FLiNG trainers are native hosts that start the .NET CLR
//! (`CLRCreateInstance`) to run a WPF window. Proton prefixes have no real
//! .NET, so Wine runs them on Wine Mono. The Wine Mono 11.2.0 shipped with
//! Proton 11.0 breaks their window (Win32Exception "Invalid window handle"):
//! the trainer never attaches and quits ~10s later with exit 0. Wine Mono
//! 11.3.0 fixes it.
//!
//! Wine prefers a runtime at `C:\windows\mono\mono-2.0` over Proton's shared
//! one and picks it per process, so placing 11.3.0 there fixes trainers even
//! while the game is running. Fling marks the copy it installs and removes it
//! once the game's Proton ships a working Wine Mono, so it never shadows a
//! newer runtime.
use crate::{config::Config, error::Error, steam};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const CLR_HOST_MARKER: &[u8] = b"CLRCreateInstance";
/// Wine Mono releases that cannot run FLiNG trainers.
const BROKEN: &[&str] = &["11.2.0"];
const VERSION: &str = "11.3.0";
const URL: &str = "https://github.com/wine-mono/wine-mono/releases/download/wine-mono-11.3.0/wine-mono-11.3.0-x86.tar.xz";
const SHA: &str = "54a1b0111c3fe4b785eae688af94d27e64995707ad648b6fee8000381b80d298";
const ARCHIVE: &str = "wine-mono-11.3.0-x86.tar.xz";
const TOP_DIR: &str = "wine-mono-11.3.0";
const MARKER: &str = ".fling-wine-mono";

/// True when the trainer hosts the .NET runtime.
pub fn trainer_hosts_clr(exe: &Path) -> bool {
    fs::read(exe).is_ok_and(|data| {
        data.windows(CLR_HOST_MARKER.len())
            .any(|window| window == CLR_HOST_MARKER)
    })
}

/// The game's compatdata directory, looked up in its own library first.
pub fn compatdata(config: &Config, game: &steam::Game) -> Option<PathBuf> {
    [PathBuf::from(&game.library_path), config.steam_root.clone()]
        .into_iter()
        .map(|library| library.join(format!("steamapps/compatdata/{}", game.appid)))
        .find(|path| path.join("pfx").is_dir())
}

/// Wine Mono versions shipped by the Proton that last ran this prefix.
///
/// Proton records its fonts directory (`<proton>/files/share/fonts/`) on the
/// second line of `config_info`; its runtimes sit in `files/share/wine/mono`.
pub fn proton_mono_versions(compatdata: &Path) -> Vec<String> {
    let Some(fonts) = fs::read_to_string(compatdata.join("config_info"))
        .ok()
        .and_then(|info| info.lines().nth(1).map(PathBuf::from))
    else {
        return Vec::new();
    };
    let Some(share) = fonts.parent() else {
        return Vec::new();
    };
    let mut versions: Vec<String> = fs::read_dir(share.join("wine/mono"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()?
                .strip_prefix("wine-mono-")
                .map(str::to_owned)
        })
        .collect();
    versions.sort();
    versions
}

fn local_runtime(compatdata: &Path) -> PathBuf {
    compatdata.join("pfx/drive_c/windows/mono/mono-2.0")
}

fn is_managed(runtime: &Path) -> bool {
    !runtime.is_symlink() && runtime.join(MARKER).is_file()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The trainer does not use .NET, or the game has no prefix yet.
    NotNeeded,
    /// The game's Proton ships a working Wine Mono.
    NotAffected,
    /// A Wine Mono that fling did not install is already in the prefix.
    Foreign,
    AlreadyInstalled,
    Installed,
    /// Fling's copy was removed because Proton no longer needs it.
    Removed,
}

/// Puts a working Wine Mono into the game's prefix when its Proton ships a
/// broken one and its trainer needs .NET; removes fling's copy otherwise.
pub fn ensure(config: &Config, game: &steam::Game) -> Result<Outcome, Error> {
    let Some(compatdata) = compatdata(config, game) else {
        return Ok(Outcome::NotNeeded);
    };
    let runtime = local_runtime(&compatdata);
    let needed = steam::find_trainer(config, game.appid).is_some_and(|exe| trainer_hosts_clr(&exe))
        && proton_mono_versions(&compatdata)
            .iter()
            .any(|version| BROKEN.contains(&version.as_str()));
    if !needed {
        if is_managed(&runtime) {
            fs::remove_dir_all(&runtime)?;
            return Ok(Outcome::Removed);
        }
        return Ok(Outcome::NotAffected);
    }
    if is_managed(&runtime) {
        return Ok(Outcome::AlreadyInstalled);
    }
    if runtime.exists() || runtime.is_symlink() {
        return Ok(Outcome::Foreign);
    }
    let archive = cached_archive(config)?;
    install(&archive, &runtime)?;
    Ok(Outcome::Installed)
}

/// Prints what `ensure` did; errors are warnings because the trainer may
/// still work.
pub fn ensure_and_report(config: &Config, game: &steam::Game) {
    match ensure(config, game) {
        Ok(Outcome::Installed) => println!(
            ">>> Installed Wine Mono {VERSION} into the game's prefix (Proton 11.0's breaks trainers) ✓"
        ),
        Ok(Outcome::Removed) => {
            println!(">>> Removed fling's Wine Mono override — this Proton's runtime works")
        }
        Ok(_) => {}
        Err(error) => println!(">>> WARNING: could not install Wine Mono {VERSION}: {error}"),
    }
}

fn sha256(path: &Path) -> Option<String> {
    fs::read(path)
        .ok()
        .map(|data| format!("{:x}", Sha256::digest(data)))
}

fn cached_archive(config: &Config) -> Result<PathBuf, Error> {
    let cache = config.home.join(".cache/fling");
    let archive = cache.join(ARCHIVE);
    if sha256(&archive).as_deref() == Some(SHA) {
        return Ok(archive);
    }
    fs::create_dir_all(&cache)?;
    let partial = cache.join(format!("{ARCHIVE}.{}.part", std::process::id()));
    println!(">>> Downloading Wine Mono {VERSION}...");
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
            "134217728",
            URL,
            "-o",
        ])
        .arg(&partial)
        .status()?;
    if !status.success() || sha256(&partial).as_deref() != Some(SHA) {
        let _ = fs::remove_file(&partial);
        return Err(Error::Network(format!(
            "Wine Mono {VERSION} download failed or did not match its checksum"
        )));
    }
    fs::rename(&partial, &archive)?;
    Ok(archive)
}

/// Accepts regular files, directories and relative symlinks that stay under
/// the release's top directory, as `tar -tvJf` lists them.
pub fn listing_is_safe(verbose_listing: &str) -> bool {
    verbose_listing.lines().all(|line| {
        let kind = line.chars().next();
        let Some(entry) = line.split_whitespace().nth(5) else {
            return false;
        };
        let entry = &line[line.find(entry).unwrap_or(0)..];
        match kind {
            Some('-' | 'd') => stays_inside(&[], entry),
            Some('l') => {
                let Some((link, target)) = entry.split_once(" -> ") else {
                    return false;
                };
                let mut parent: Vec<&str> =
                    link.split('/').filter(|part| !part.is_empty()).collect();
                parent.pop();
                stays_inside(&[], link) && !target.starts_with('/') && stays_inside(&parent, target)
            }
            _ => false,
        }
    })
}

/// True when `path`, resolved from `base`, stays under the top directory.
fn stays_inside(base: &[&str], path: &str) -> bool {
    let mut parts: Vec<&str> = base.to_vec();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
                if parts.is_empty() {
                    return false;
                }
            }
            part => parts.push(part),
        }
    }
    parts.first() == Some(&TOP_DIR)
}

fn install(archive: &Path, runtime: &Path) -> Result<(), Error> {
    let listing = Command::new("tar").arg("-tvJf").arg(archive).output()?;
    if !listing.status.success() || !listing_is_safe(&String::from_utf8_lossy(&listing.stdout)) {
        return Err(Error::InvalidPayload(
            "Wine Mono archive contains unsafe entries".into(),
        ));
    }
    let parent = runtime
        .parent()
        .ok_or_else(|| Error::Message("invalid prefix path".into()))?;
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(".fling-wine-mono-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir(&staging)?;
    let result = (|| {
        let status = Command::new("tar")
            .args(["-xJf"])
            .arg(archive)
            .args(["--no-same-owner", "--no-same-permissions", "-C"])
            .arg(&staging)
            .status()?;
        if !status.success() {
            return Err(Error::Message("Wine Mono archive extraction failed".into()));
        }
        let extracted = staging.join(TOP_DIR);
        fs::write(extracted.join(MARKER), format!("{VERSION}\n"))?;
        match fs::rename(&extracted, runtime) {
            // Another fling run (the watcher, say) installed it first.
            Err(_) if is_managed(runtime) => Ok(()),
            result => Ok(result?),
        }
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}
