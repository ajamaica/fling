use crate::{
    archive,
    config::Config,
    error::{Error, json_failure},
    fs_safe::Dir,
    runtime, steam,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) Firefox/128.0";
#[derive(Serialize)]
struct Success {
    schema_version: u8,
    success: bool,
    operation: &'static str,
    appid: u32,
    name: String,
    message: &'static str,
    trainer_path: String,
    restart_required: bool,
}
#[derive(Serialize)]
struct Removed {
    schema_version: u8,
    success: bool,
    operation: &'static str,
    appid: u32,
    name: String,
    message: &'static str,
    restart_required: bool,
}
fn base_slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c)
        } else if c == '&' {
            s.push_str("and")
        } else if !matches!(c, '\'' | '’' | ':' | '.' | ',' | '®' | '™') && !s.ends_with('-') {
            s.push('-')
        }
    }
    s.trim_matches('-').to_owned()
}

fn replace_numerals(value: &str, replacements: &[(&str, &str)]) -> String {
    let mut words: Vec<_> = value.split('-').map(str::to_owned).collect();
    for (from, replacement) in replacements {
        if let Some(word) = words.iter_mut().find(|word| word == from) {
            *word = (*replacement).to_owned();
        }
    }
    words.join("-")
}

fn slug_candidates(name: &str) -> Vec<String> {
    let base = base_slug(name);
    let variants = [
        base.clone(),
        replace_numerals(&base, &[("iii", "3"), ("ii", "2"), ("iv", "4")]),
        replace_numerals(&base, &[("3", "iii"), ("2", "ii"), ("4", "iv")]),
    ];
    let mut candidates: Vec<_> = variants
        .into_iter()
        .map(|variant| format!("{variant}-trainer"))
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn find_trainer_page_with<F>(name: &str, mut probe: F) -> Result<Option<String>, Error>
where
    F: FnMut(&str) -> Result<bool, Error>,
{
    for candidate in slug_candidates(name) {
        let page = format!("https://flingtrainer.com/trainer/{candidate}/");
        if probe(&page)? {
            return Ok(Some(page));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod slug_tests {
    use super::slug_candidates;

    #[test]
    fn candidates_remove_apostrophes_and_preserve_numbered_titles() {
        assert_eq!(
            slug_candidates("Baldur's Gate 3"),
            vec!["baldurs-gate-3-trainer", "baldurs-gate-iii-trainer"]
        );
    }

    #[test]
    fn candidates_include_deduplicated_roman_and_arabic_variants() {
        assert_eq!(
            slug_candidates("Game III: Redux"),
            vec!["game-3-redux-trainer", "game-iii-redux-trainer"]
        );
        assert_eq!(
            slug_candidates("Game II & IV"),
            vec!["game-2-and-4-trainer", "game-ii-and-iv-trainer"]
        );
    }

    #[test]
    fn numeral_substitutions_change_only_the_first_matching_word() {
        assert_eq!(
            slug_candidates("Game II II IV IV"),
            vec!["game-2-ii-4-iv-trainer", "game-ii-ii-iv-iv-trainer",]
        );
    }

    #[test]
    fn page_lookup_tries_candidates_until_one_succeeds_without_network() {
        let mut attempted = Vec::new();
        let page = super::find_trainer_page_with("Baldur's Gate 3", |url| {
            attempted.push(url.to_owned());
            Ok(url.ends_with("/baldurs-gate-iii-trainer/"))
        })
        .expect("lookup")
        .expect("matching candidate");
        assert_eq!(
            attempted,
            vec![
                "https://flingtrainer.com/trainer/baldurs-gate-3-trainer/",
                "https://flingtrainer.com/trainer/baldurs-gate-iii-trainer/",
            ]
        );
        assert_eq!(page, attempted[1]);
    }

    #[test]
    fn page_lookup_uses_bash_sort_order() {
        let mut attempted = Vec::new();
        let page = super::find_trainer_page_with("Game III", |url| {
            attempted.push(url.to_owned());
            Ok(true)
        })
        .expect("lookup")
        .expect("matching candidate");
        assert!(page.ends_with("/game-3-trainer/"));
        assert_eq!(attempted.len(), 1);
    }
}
fn run_curl(args: &[&str], tail: &Path) -> Result<std::process::ExitStatus, Error> {
    Command::new("curl")
        .args(args)
        .arg(tail)
        .status()
        .map_err(|error| command_error("curl", error))
}
fn command_error(name: &str, error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        Error::DependencyMissing(name.into())
    } else {
        Error::Io(error)
    }
}
fn require_command(name: &str) -> Result<(), Error> {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|_| ())
        .map_err(|error| command_error(name, error))
}
fn download(game: &steam::Game, dest: &Path) -> Result<(String, String), Error> {
    let null = Path::new("/dev/null");
    let page = find_trainer_page_with(&game.name, |candidate_page| {
        let out = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail",
                "--location",
                "--connect-timeout",
                "10",
                "--max-time",
                "30",
                "-A",
                UA,
                "-o",
            ])
            .arg(null)
            .args(["-w", "%{http_code}", candidate_page])
            .output()
            .map_err(|error| command_error("curl", error))?;
        let code = String::from_utf8_lossy(&out.stdout);
        if code == "200" {
            Ok(true)
        } else if code == "404" {
            Ok(false)
        } else {
            Err(Error::Network(format!(
                "trainer page request failed (HTTP {code})"
            )))
        }
    })?;
    let page =
        page.ok_or_else(|| Error::TrainerNotFound("No FLiNG trainer found for this game".into()))?;
    let html = Command::new("curl")
        .args([
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--connect-timeout",
            "10",
            "--max-time",
            "30",
            "-A",
            UA,
            &page,
        ])
        .output()
        .map_err(|error| command_error("curl", error))?;
    if !html.status.success() {
        return Err(Error::Network("trainer page download failed".into()));
    }
    let text = String::from_utf8_lossy(&html.stdout);
    let Some(start) = text.find("https://flingtrainer.com/downloads/") else {
        return Err(Error::TrainerNotFound(
            "No FLiNG trainer found for this game".into(),
        ));
    };
    let Some(url) = text[start..]
        .split(['\"', '\'', '<', '>'])
        .next()
        .map(str::to_owned)
    else {
        return Err(Error::TrainerNotFound(
            "No FLiNG trainer found for this game".into(),
        ));
    };
    let status = run_curl(
        &[
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--connect-timeout",
            "15",
            "--max-time",
            "240",
            "--max-filesize",
            "536870912",
            "-A",
            UA,
            &url,
            "-o",
        ],
        dest,
    )?;
    if !status.success() {
        return Err(Error::Network("Trainer download failed".into()));
    }
    Ok((page, url))
}
fn is_symlink(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
}

#[derive(Deserialize, Serialize)]
struct TrainerTransaction {
    schema_version: u8,
    target: String,
    stage: String,
    backup: String,
}

fn owned_transaction_name(value: &str, prefixes: &[&str]) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && !value.contains('\\')
        && prefixes.iter().any(|prefix| value.starts_with(prefix))
}

fn commit_staged_trainer(
    target: &Path,
    stage: &Path,
    fail_after_backup: bool,
) -> Result<(), Error> {
    let parent = target
        .parent()
        .ok_or_else(|| Error::Message("unsafe trainer target".into()))?;
    if stage.parent() != Some(parent) {
        return Err(Error::Message("unsafe trainer stage".into()));
    }
    let target_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::Message("unsafe trainer target".into()))?;
    let stage_name = stage
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| Error::Message("unsafe trainer stage".into()))?;
    if !owned_transaction_name(stage_name, &[".fling-install-"]) {
        return Err(Error::Message("unsafe trainer stage".into()));
    }
    let appid = target_name
        .split_once(" - ")
        .map(|(id, _)| id)
        .filter(|id| id.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| Error::Message("unsafe trainer target".into()))?;
    let directory = Dir::open_verified(parent)?;
    if !directory.is_dir(stage_name)? {
        return Err(Error::Message("unsafe trainer stage".into()));
    }
    if directory.exists(target_name)? && !directory.is_dir(target_name)? {
        return Err(Error::Message("unsafe trainer target".into()));
    }
    let marker = format!(".fling-transaction-{appid}.json");
    if directory.exists(&marker)? {
        let old: TrainerTransaction = serde_json::from_slice(&directory.read_regular(&marker)?)?;
        if !owned_transaction_name(&old.target, &[&format!("{appid} - ")])
            || !owned_transaction_name(&old.stage, &[".fling-install-"])
            || !owned_transaction_name(&old.backup, &[".fling-backup-"])
        {
            return Err(Error::Message("unsafe trainer transaction marker".into()));
        }
        if !directory.exists(&old.target)? && directory.is_dir(&old.backup).unwrap_or(false) {
            directory.rename(&old.backup, &old.target)?;
        } else if directory.is_dir(&old.target).unwrap_or(false)
            && directory.is_dir(&old.backup).unwrap_or(false)
        {
            directory.remove_tree(&old.backup)?;
        }
        if old.stage != stage_name && directory.is_dir(&old.stage).unwrap_or(false) {
            directory.remove_tree(&old.stage)?;
        }
        directory.unlink(&marker)?;
        directory.sync()?;
    }
    let backup = format!(".fling-backup-{appid}.{}", std::process::id());
    if directory.exists(&backup)? {
        return Err(Error::Message("trainer backup collision".into()));
    }
    let transaction = TrainerTransaction {
        schema_version: 1,
        target: target_name.into(),
        stage: stage_name.into(),
        backup: backup.clone(),
    };
    let marker_temp =
        directory.write_exclusive(&format!("{marker}."), &serde_json::to_vec(&transaction)?)?;
    directory.rename(&marker_temp, &marker)?;
    directory.sync()?;
    let had_target = directory.exists(target_name)?;
    if had_target {
        directory.rename(target_name, &backup)?;
        directory.sync()?;
    }
    let commit = if fail_after_backup {
        Err(Error::Message("injected trainer commit failure".into()))
    } else {
        directory
            .rename(stage_name, target_name)
            .map_err(Error::from)
    };
    if let Err(error) = commit {
        if had_target && !directory.exists(target_name)? && directory.exists(&backup)? {
            directory.rename(&backup, target_name)?;
        }
        directory.unlink(&marker)?;
        directory.sync()?;
        return Err(error);
    }
    directory.sync()?;
    if directory.exists(&backup)? {
        directory.remove_tree(&backup)?;
    }
    directory.unlink(&marker)?;
    directory.sync()?;
    Ok(())
}

fn commit_or_restore_runtime(
    config: &Config,
    appid: u32,
    target: &Path,
    stage: &Path,
    snapshot: runtime::Snapshot,
    fail_after_backup: bool,
) -> Result<(), Error> {
    if let Err(commit_error) = commit_staged_trainer(target, stage, fail_after_backup) {
        if let Err(restore_error) = runtime::restore(config, appid, snapshot) {
            return Err(Error::Message(format!(
                "{commit_error}; runtime rollback failed: {restore_error}"
            )));
        }
        return Err(commit_error);
    }
    Ok(())
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum RemovalPhase {
    Staged,
    Committed,
}

#[derive(Deserialize, Serialize)]
struct RemovalTransaction {
    schema_version: u8,
    appid: u32,
    original: String,
    tombstone: String,
    phase: RemovalPhase,
}

fn preflight_tree(path: &Path) -> Result<(), Error> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        entry
            .file_name()
            .to_str()
            .ok_or_else(|| Error::Message("non-UTF8 tree entry".into()))?;
        let kind = entry.file_type()?;
        if kind.is_symlink() || (!kind.is_dir() && !kind.is_file()) {
            return Err(Error::Message("unsafe trainer tree entry".into()));
        }
        if kind.is_dir() {
            preflight_tree(&entry.path())?;
        }
    }
    Ok(())
}

fn removal_names(appid: u32) -> (String, String) {
    (
        format!(".fling-remove-{appid}.json"),
        format!(".fling-remove-{appid}"),
    )
}

fn write_removal_marker(root: &Dir, marker: &str, tx: &RemovalTransaction) -> Result<(), Error> {
    let temporary = root.write_exclusive(&format!("{marker}."), &serde_json::to_vec(tx)?)?;
    root.rename(&temporary, marker)?;
    root.sync()?;
    Ok(())
}

fn read_removal_marker(root: &Dir, appid: u32) -> Result<Option<RemovalTransaction>, Error> {
    let (marker, tombstone) = removal_names(appid);
    if !root.exists(&marker)? {
        if root.exists(&tombstone)? {
            return Err(Error::Message("orphan trainer removal tombstone".into()));
        }
        return Ok(None);
    }
    let tx: RemovalTransaction = serde_json::from_slice(&root.read_regular(&marker)?)?;
    if tx.schema_version != 1
        || tx.appid != appid
        || tx.tombstone != tombstone
        || !owned_transaction_name(&tx.original, &[&format!("{appid} - ")])
    {
        return Err(Error::Message("unsafe trainer removal marker".into()));
    }
    Ok(Some(tx))
}

fn recover_removal(config: &Config, appid: u32) -> Result<(), Error> {
    if !config.trainers.exists() {
        return Ok(());
    }
    let root = Dir::open_verified(&config.trainers)?;
    let (marker, _) = removal_names(appid);
    let Some(tx) = read_removal_marker(&root, appid)? else {
        return Ok(());
    };
    match tx.phase {
        RemovalPhase::Staged => {
            runtime::reconcile_removal(config, appid, false)?;
            let has_original = root.exists(&tx.original)?;
            let has_tombstone = root.exists(&tx.tombstone)?;
            match (has_original, has_tombstone) {
                (false, true) => root.rename(&tx.tombstone, &tx.original)?,
                (true, false) => {}
                _ => return Err(Error::Message("unsafe staged removal state".into())),
            }
            root.sync()?;
            root.unlink(&marker)?;
            root.sync()?;
        }
        RemovalPhase::Committed => {
            runtime::reconcile_removal(config, appid, true)?;
            if root.exists(&tx.original)? {
                return Err(Error::Message("committed removal has live trainer".into()));
            }
            if root.exists(&tx.tombstone)? {
                preflight_tree(&config.trainers.join(&tx.tombstone))?;
                root.remove_tree(&tx.tombstone)?;
                root.sync()?;
            }
            root.unlink(&marker)?;
            root.sync()?;
        }
    }
    Ok(())
}

fn remove_transaction(
    config: &Config,
    appid: u32,
    directory_name: &str,
    fail_runtime_removal: bool,
    fail_trainer_finalization: bool,
) -> Result<(), Error> {
    let root = Dir::open_verified(&config.trainers)?;
    recover_removal(config, appid)?;
    let (marker, tombstone) = removal_names(appid);
    if !root.is_dir(directory_name)? {
        return Err(Error::Message("trainer directory identity changed".into()));
    }
    preflight_tree(&config.trainers.join(directory_name))?;
    let runtime_snapshot = runtime::snapshot(config, appid)
        .map_err(|error| Error::Message(format!("runtime removal failed: {error}")))?;
    let mut transaction = RemovalTransaction {
        schema_version: 1,
        appid,
        original: directory_name.into(),
        tombstone: tombstone.clone(),
        phase: RemovalPhase::Staged,
    };
    write_removal_marker(&root, &marker, &transaction)?;
    root.rename(directory_name, &tombstone)?;
    root.sync()?;

    let runtime_removal = if fail_runtime_removal {
        Err(Error::Message("injected runtime removal failure".into()))
    } else {
        runtime::stage_removal(config, appid)
    };
    if let Err(error) = runtime_removal {
        let runtime_restore = runtime::restore(config, appid, runtime_snapshot);
        let trainer_restore = root
            .rename(&tombstone, directory_name)
            .and_then(|()| root.sync());
        if runtime_restore.is_ok() && trainer_restore.is_ok() {
            root.unlink(&marker)?;
            root.sync()?;
        }
        return match (runtime_restore, trainer_restore) {
            (Ok(()), Ok(())) => Err(Error::Message(format!("runtime removal failed: {error}"))),
            (runtime, trainer) => Err(Error::Message(format!(
                "runtime removal failed: {error}; rollback failed: runtime={runtime:?}, trainer={trainer:?}"
            ))),
        };
    }

    transaction.phase = RemovalPhase::Committed;
    write_removal_marker(&root, &marker, &transaction)?;
    runtime::reconcile_removal(config, appid, true)?;
    let finalization = if fail_trainer_finalization {
        Err(std::io::Error::other(
            "injected trainer finalization failure",
        ))
    } else {
        root.remove_tree(&tombstone).and_then(|()| root.sync())
    };
    if finalization.is_err() {
        return Ok(());
    }
    root.unlink(&marker)?;
    root.sync()?;
    Ok(())
}
fn install_inner(config: &Config, appid: u32) -> Result<(steam::Game, PathBuf), Error> {
    let game = steam::game(config, appid).ok_or_else(|| Error::Message("game_missing".into()))?;
    if is_symlink(&config.trainers) {
        return Err(Error::Message("unsafe_path".into()));
    }
    for dependency in ["curl", "file", "python3"] {
        require_command(dependency)?;
    }
    fs::create_dir_all(&config.trainers)?;
    let target = config
        .trainers
        .join(format!("{} - {}", appid, steam::safe_name(&game.name)));
    if is_symlink(&target) {
        return Err(Error::Message("unsafe_path".into()));
    }
    let stage_guard = tempfile::Builder::new()
        .prefix(&format!(".fling-install-{appid}-"))
        .tempdir_in(&config.trainers)?;
    let stage = stage_guard.path().to_path_buf();
    let payload = stage.join("payload");
    let result = (|| {
        let (page, url) = download(&game, &payload)?;
        let detected = Command::new("file")
            .args(["-b"])
            .arg(&payload)
            .output()
            .map_err(|error| command_error("file", error))?;
        let kind = String::from_utf8_lossy(&detected.stdout);
        let trainer = stage.join("Trainer.exe");
        if kind.contains("Zip archive data") {
            if !archive::validate(&payload) {
                return Err(Error::InvalidPayload(
                    "trainer archive is invalid or unsafe".into(),
                ));
            }
            archive::extract_largest_exe(&payload, &trainer)?;
        } else if kind.contains("PE32") {
            fs::rename(&payload, &trainer)?
        } else {
            return Err(Error::InvalidPayload("invalid_file".into()));
        }
        let bytes = fs::read(&trainer)?;
        let metadata = serde_json::json!({"schema_version":1,"appid":appid,"game_name":game.name,"page_url":page,"download_url":url,"sha256":format!("{:x}",Sha256::digest(&bytes)),"installed_at":format!("{}Z",SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())});
        fs::write(
            stage.join("trainer-metadata.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;
        let runtime_snapshot = if appid == 3_357_650 {
            let snapshot = runtime::snapshot(config, appid)?;
            runtime::install(config, appid)?;
            snapshot
        } else {
            runtime::Snapshot::NotApplicable
        };
        commit_or_restore_runtime(config, appid, &target, &stage, runtime_snapshot, false)?;
        Ok((game, target.join("Trainer.exe")))
    })();
    drop(stage_guard);
    result
}
pub fn install_json(config: &Config, arg: &str) {
    let Ok(appid) = arg.parse() else {
        json_failure("install", 0, 2, "invalid_args", "appid must be numeric")
    };
    match install_inner(config, appid) {
        Ok((g, p)) => match serde_json::to_string(&Success {
            schema_version: 1,
            success: true,
            operation: "install",
            appid,
            name: g.name,
            message: "Trainer installed successfully",
            trainer_path: p.to_string_lossy().into(),
            restart_required: false,
        }) {
            Ok(value) => println!("{value}"),
            Err(error) => json_failure("install", appid, 1, "general_error", error.to_string()),
        },
        Err(Error::Message(m)) if m == "game_missing" => json_failure(
            "install",
            appid,
            3,
            "game_missing",
            "Installed Steam game not found",
        ),
        Err(Error::Message(m)) if m == "unsafe_path" => json_failure(
            "install",
            appid,
            9,
            "unsafe_path",
            "Refusing unsafe trainer path",
        ),
        Err(Error::DependencyMissing(name)) => json_failure(
            "install",
            appid,
            8,
            "dependency_missing",
            format!("Missing required dependency: {name}"),
        ),
        Err(Error::Network(message)) => json_failure("install", appid, 5, "network_error", message),
        Err(Error::TrainerNotFound(message)) => {
            json_failure("install", appid, 4, "trainer_not_found", message)
        }
        Err(Error::InvalidPayload(message)) => {
            json_failure("install", appid, 6, "invalid_file", message)
        }
        Err(e) if appid == 3_357_650 => json_failure(
            "install",
            appid,
            11,
            "runtime_support_failed",
            e.to_string(),
        ),
        Err(e) => json_failure("install", appid, 6, "invalid_file", e.to_string()),
    }
}
pub fn remove_json(config: &Config, arg: &str) {
    let Ok(appid) = arg.parse() else {
        json_failure("remove", 0, 2, "invalid_args", "appid must be numeric")
    };
    let Some(g) = steam::game(config, appid) else {
        json_failure(
            "remove",
            appid,
            3,
            "game_missing",
            "Installed Steam game not found",
        )
    };
    if is_symlink(&config.trainers) {
        json_failure(
            "remove",
            appid,
            9,
            "unsafe_path",
            "Refusing unsafe trainer path",
        )
    }
    if let Err(error) = recover_removal(config, appid) {
        json_failure("remove", appid, 9, "unsafe_path", error.to_string())
    }
    if let Ok(entries) = fs::read_dir(&config.trainers) {
        for e in entries.flatten() {
            if e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{appid} - "))
                && is_symlink(&e.path())
            {
                json_failure(
                    "remove",
                    appid,
                    9,
                    "unsafe_path",
                    "Refusing unsafe trainer path",
                )
            }
        }
    }
    let exe = match steam::unique_trainer(config, appid) {
        Ok(exe) => exe,
        Err(steam::TrainerLookupError::Missing) => json_failure(
            "remove",
            appid,
            7,
            "local_trainer_missing",
            "No local trainer is installed",
        ),
        Err(steam::TrainerLookupError::Multiple) => json_failure(
            "remove",
            appid,
            9,
            "unsafe_path",
            "Multiple trainer directories found; refusing removal",
        ),
        Err(steam::TrainerLookupError::Unsafe) => json_failure(
            "remove",
            appid,
            9,
            "unsafe_path",
            "Refusing unsafe trainer path",
        ),
    };
    let Some(directory) = exe.parent() else {
        json_failure(
            "remove",
            appid,
            9,
            "unsafe_path",
            "Refusing unsafe trainer path",
        )
    };
    let Some(directory_name) = directory.file_name().and_then(|name| name.to_str()) else {
        json_failure(
            "remove",
            appid,
            9,
            "unsafe_path",
            "Refusing unsafe trainer path",
        )
    };
    if let Err(error) = remove_transaction(config, appid, directory_name, false, false) {
        if error.to_string().starts_with("runtime removal failed:") {
            json_failure(
                "remove",
                appid,
                12,
                "runtime_support_conflict",
                error.to_string(),
            )
        }
        json_failure("remove", appid, 9, "unsafe_path", error.to_string())
    }
    match serde_json::to_string(&Removed {
        schema_version: 1,
        success: true,
        operation: "remove",
        appid,
        name: g.name,
        message: "Trainer removed successfully",
        restart_required: false,
    }) {
        Ok(value) => println!("{value}"),
        Err(error) => json_failure("remove", appid, 1, "general_error", error.to_string()),
    }
}
pub fn legacy_get(config: &Config, arg: &str) -> Result<(), Error> {
    let appid = resolve(config, arg)?.appid;
    install_inner(config, appid)?;
    println!(">>> Trainer installed successfully");
    Ok(())
}
pub fn resolve(config: &Config, q: &str) -> Result<steam::Game, Error> {
    let all = steam::games(config);
    let hits: Vec<_> = if let Ok(id) = q.parse::<u32>() {
        all.into_iter().filter(|g| g.appid == id).collect()
    } else {
        let q = q.to_lowercase();
        all.into_iter()
            .filter(|g| g.name.to_lowercase().contains(&q))
            .collect()
    };
    match hits.as_slice() {
        [g] => Ok(g.clone()),
        [] => Err(Error::Message(format!(
            "no installed Steam game matches '{q}' (try: fling list)"
        ))),
        _ => Err(Error::Message("be more specific".into())),
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;

    fn removal_fixture() -> (tempfile::TempDir, Config, String) {
        let temp = tempfile::tempdir().expect("tempdir");
        let steam = temp.path().join("steam");
        let game = steam.join("steamapps/common/Game");
        fs::create_dir_all(&game).expect("game");
        fs::write(
            steam.join("steamapps/appmanifest_3357650.acf"),
            r#""appid" "3357650" "name" "Game" "installdir" "Game""#,
        )
        .expect("manifest");
        let trainers = temp.path().join("Trainers");
        fs::create_dir(&trainers).expect("trainers");
        let directory_name = "3357650 - Game".to_owned();
        fs::create_dir(trainers.join(&directory_name)).expect("trainer directory");
        fs::write(
            trainers.join(&directory_name).join("Trainer.exe"),
            b"trainer",
        )
        .expect("trainer");
        let dll = b"MZ-runtime";
        fs::write(game.join("dinput8.dll"), dll).expect("runtime");
        fs::write(
            game.join(".fling-reframework.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1, "appid": 3357650, "component": "REFramework",
                "installed_file": "dinput8.dll", "sha256": format!("{:x}", Sha256::digest(dll)),
            }))
            .expect("metadata"),
        )
        .expect("metadata file");
        let config = Config {
            home: temp.path().join("home"),
            steam_root: steam,
            trainers,
            proc_root: temp.path().join("proc"),
        };
        (temp, config, directory_name)
    }

    fn assert_removal_rolled_back(config: &Config, directory_name: &str) {
        assert!(
            config
                .trainers
                .join(directory_name)
                .join("Trainer.exe")
                .is_file()
        );
        let game = config.steam_root.join("steamapps/common/Game");
        assert!(game.join("dinput8.dll").is_file());
        assert!(game.join(".fling-reframework.json").is_file());
    }

    #[test]
    fn runtime_removal_failure_restores_staged_trainer() {
        let (_temp, config, directory_name) = removal_fixture();
        assert!(remove_transaction(&config, 3_357_650, &directory_name, true, false).is_err());
        assert_removal_rolled_back(&config, &directory_name);
    }

    #[test]
    fn trainer_finalization_failure_leaves_committed_gc_for_retry() {
        let (_temp, config, directory_name) = removal_fixture();
        remove_transaction(&config, 3_357_650, &directory_name, false, true)
            .expect("logical removal committed");
        assert!(!config.trainers.join(&directory_name).exists());
        assert!(config.trainers.join(".fling-remove-3357650").exists());
        assert!(config.trainers.join(".fling-remove-3357650.json").exists());
        recover_removal(&config, 3_357_650).expect("retry garbage collection");
        assert!(!config.trainers.join(".fling-remove-3357650").exists());
        assert!(!config.trainers.join(".fling-remove-3357650.json").exists());
    }

    #[test]
    fn committed_gc_retains_trainer_state_while_runtime_library_is_unavailable() {
        let (_temp, mut config, directory_name) = removal_fixture();
        remove_transaction(&config, 3_357_650, &directory_name, false, true)
            .expect("logical removal committed");
        let available_steam = config.steam_root.clone();
        config.steam_root = config.home.join("temporarily-unavailable-steam");

        assert!(recover_removal(&config, 3_357_650).is_err());
        assert!(config.trainers.join(".fling-remove-3357650").exists());
        assert!(config.trainers.join(".fling-remove-3357650.json").exists());

        config.steam_root = available_steam;
        recover_removal(&config, 3_357_650).expect("retry after library returns");
        assert!(!config.trainers.join(".fling-remove-3357650").exists());
        assert!(!config.trainers.join(".fling-remove-3357650.json").exists());
    }

    #[test]
    fn interrupted_trainer_staging_is_recovered_before_retry() {
        let (_temp, config, directory_name) = removal_fixture();
        fs::rename(
            config.trainers.join(&directory_name),
            config.trainers.join(".fling-remove-3357650"),
        )
        .expect("simulate interrupted staging");
        fs::write(
            config.trainers.join(".fling-remove-3357650.json"),
            serde_json::to_vec(&RemovalTransaction {
                schema_version: 1,
                appid: 3_357_650,
                original: directory_name.clone(),
                tombstone: ".fling-remove-3357650".into(),
                phase: RemovalPhase::Staged,
            })
            .expect("marker"),
        )
        .expect("marker file");
        assert!(remove_transaction(&config, 3_357_650, &directory_name, true, false).is_err());
        assert_removal_rolled_back(&config, &directory_name);
    }

    #[test]
    fn trainer_commit_failure_restores_old_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("42 - Game");
        let stage = temp.path().join(".fling-install-42-stage");
        fs::create_dir(&target).expect("target");
        fs::write(target.join("Trainer.exe"), b"old").expect("old trainer");
        fs::create_dir(&stage).expect("stage");
        fs::write(stage.join("Trainer.exe"), b"new").expect("new trainer");
        assert!(commit_staged_trainer(&target, &stage, true).is_err());
        assert_eq!(
            fs::read(target.join("Trainer.exe")).expect("preserved trainer"),
            b"old"
        );
    }

    #[test]
    fn interrupted_trainer_commit_restores_backup_to_recorded_old_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old_target = temp.path().join("42 - Old Name");
        let new_target = temp.path().join("42 - New Name");
        let old_stage = temp.path().join(".fling-install-42-old-stage");
        let new_stage = temp.path().join(".fling-install-42-new-stage");
        let backup = temp.path().join(".fling-backup-42.old");
        fs::create_dir(&backup).expect("backup");
        fs::write(backup.join("Trainer.exe"), b"old").expect("old trainer");
        fs::create_dir(&old_stage).expect("old stage");
        fs::write(old_stage.join("Trainer.exe"), b"abandoned").expect("abandoned trainer");
        fs::create_dir(&new_stage).expect("new stage");
        fs::write(new_stage.join("Trainer.exe"), b"new").expect("new trainer");
        fs::write(
            temp.path().join(".fling-transaction-42.json"),
            serde_json::to_vec(&TrainerTransaction {
                schema_version: 1,
                target: "42 - Old Name".into(),
                stage: ".fling-install-42-old-stage".into(),
                backup: ".fling-backup-42.old".into(),
            })
            .expect("transaction"),
        )
        .expect("marker");

        commit_staged_trainer(&new_target, &new_stage, false).expect("recover and commit");

        assert_eq!(
            fs::read(old_target.join("Trainer.exe")).expect("restored old trainer"),
            b"old"
        );
        assert_eq!(
            fs::read(new_target.join("Trainer.exe")).expect("committed new trainer"),
            b"new"
        );
        assert!(!backup.exists());
        assert!(!old_stage.exists());
        assert!(!temp.path().join(".fling-transaction-42.json").exists());
    }

    #[test]
    fn trainer_commit_refuses_symlink_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("outside");
        let target = temp.path().join("42 - Game");
        let stage = temp.path().join(".fling-install-42-stage");
        std::os::unix::fs::symlink(outside.path(), &target).expect("target symlink");
        fs::create_dir(&stage).expect("stage");
        assert!(commit_staged_trainer(&target, &stage, false).is_err());
        assert!(outside.path().is_dir());
    }

    #[test]
    fn trainer_commit_failure_restores_old_runtime_too() {
        let temp = tempfile::tempdir().expect("tempdir");
        let steam = temp.path().join("steam");
        let game = steam.join("steamapps/common/Game");
        fs::create_dir_all(&game).expect("game");
        fs::write(
            steam.join("steamapps/appmanifest_3357650.acf"),
            r#""appid" "3357650" "name" "Game" "installdir" "Game""#,
        )
        .expect("manifest");
        let trainers = temp.path().join("Trainers");
        fs::create_dir(&trainers).expect("trainers");
        let config = Config {
            home: temp.path().join("home"),
            steam_root: steam,
            trainers: trainers.clone(),
            proc_root: temp.path().join("proc"),
        };
        let old_dll = b"MZ-old-runtime";
        fs::write(game.join("dinput8.dll"), old_dll).expect("old runtime");
        fs::write(
            game.join(".fling-reframework.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1, "appid": 3357650, "component": "REFramework",
                "installed_file": "dinput8.dll", "sha256": format!("{:x}", Sha256::digest(old_dll)),
            }))
            .expect("json"),
        )
        .expect("old metadata");
        let snapshot = runtime::snapshot(&config, 3_357_650).expect("snapshot");
        fs::write(game.join("dinput8.dll"), b"MZ-new-runtime").expect("new runtime");
        let new_digest = format!("{:x}", Sha256::digest(b"MZ-new-runtime"));
        fs::write(
            game.join(".fling-reframework.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1, "appid": 3357650, "component": "REFramework",
                "installed_file": "dinput8.dll", "sha256": new_digest,
            }))
            .expect("json"),
        )
        .expect("new metadata");
        let target = trainers.join("3357650 - Game");
        let stage = trainers.join(".fling-install-3357650-stage");
        fs::create_dir(&target).expect("target");
        fs::write(target.join("Trainer.exe"), b"old trainer").expect("trainer");
        fs::create_dir(&stage).expect("stage");
        assert!(
            commit_or_restore_runtime(&config, 3_357_650, &target, &stage, snapshot, true,)
                .is_err()
        );
        assert_eq!(
            fs::read(game.join("dinput8.dll")).expect("runtime"),
            old_dll
        );
        assert_eq!(
            fs::read(target.join("Trainer.exe")).expect("trainer"),
            b"old trainer"
        );
    }
}
