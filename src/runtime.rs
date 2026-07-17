use crate::{archive, config::Config, error::Error, fs_safe::Dir, steam};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
const APPID: u32 = 3_357_650;
const URL: &str = "https://github.com/praydog/REFramework-nightly/releases/download/nightly-01391-a0e9010fb0449dc9d824b5978ee759eeaf50f7c6/REFramework.zip";
const SHA: &str = "10792e2b1141c4c0e135141da5d92b281a334cea0ff9fcc3f3e482cf2df8d00f";
#[derive(Serialize, Deserialize)]
pub(crate) struct Metadata {
    #[serde(default)]
    schema_version: u8,
    appid: u32,
    component: String,
    installed_file: String,
    #[serde(default)]
    source_url: String,
    #[serde(default)]
    archive_sha256: String,
    sha256: String,
    #[serde(default)]
    installed_at: String,
}
fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn now() -> String {
    format!(
        "{}Z",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    )
}
fn curl(url: &str, out: &Path) -> Result<(), Error> {
    let s = Command::new("curl")
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
            "240",
            "--max-filesize",
            "67108864",
            url,
            "-o",
        ])
        .arg(out)
        .status()?;
    if s.success() {
        Ok(())
    } else {
        Err(Error::Message("REFramework download failed".into()))
    }
}
pub(crate) enum Snapshot {
    NotApplicable,
    Absent,
    Managed { dll: Vec<u8>, metadata: Metadata },
}

pub(crate) fn snapshot(config: &Config, appid: u32) -> Result<Snapshot, Error> {
    if appid != APPID {
        return Ok(Snapshot::NotApplicable);
    }
    let directory = steam::game_dir(config, appid)?;
    let dll = directory.exists("dinput8.dll")?;
    let metadata = directory.exists(".fling-reframework.json")?;
    if !dll && !metadata {
        return Ok(Snapshot::Absent);
    }
    if dll != metadata {
        return Err(Error::Message("managed runtime is incomplete".into()));
    }
    let bytes = directory.read_regular("dinput8.dll")?;
    let record: Metadata =
        serde_json::from_slice(&directory.read_regular(".fling-reframework.json")?)?;
    if record.appid != appid || record.sha256 != digest(&bytes) {
        return Err(Error::Message(
            "runtime support changed outside Fling".into(),
        ));
    }
    Ok(Snapshot::Managed {
        dll: bytes,
        metadata: record,
    })
}

pub(crate) fn restore(config: &Config, appid: u32, snapshot: Snapshot) -> Result<(), Error> {
    match snapshot {
        Snapshot::NotApplicable => Ok(()),
        Snapshot::Absent => remove(config, appid),
        Snapshot::Managed { dll, metadata } => {
            let directory = steam::game_dir(config, appid)?;
            install_payload(&directory, metadata, &dll, false)
        }
    }
}

fn recover_pending(directory: &Dir, appid: u32) -> Result<(), Error> {
    const PENDING: &str = ".fling-reframework.pending.json";
    if !directory.exists(PENDING)? {
        return Ok(());
    }
    let pending: Metadata = serde_json::from_slice(&directory.read_regular(PENDING)?)?;
    let matches = directory.exists("dinput8.dll")?
        && pending.appid == appid
        && pending.sha256 == digest(&directory.read_regular("dinput8.dll")?);
    if matches {
        directory.rename(PENDING, ".fling-reframework.json")?;
    } else {
        directory.unlink(PENDING)?;
    }
    directory.sync()?;
    Ok(())
}

fn install_payload(
    directory: &Dir,
    metadata: Metadata,
    dll: &[u8],
    fail_metadata_commit: bool,
) -> Result<(), Error> {
    recover_pending(directory, metadata.appid)?;
    let target_exists = directory.exists("dinput8.dll")?;
    let metadata_exists = directory.exists(".fling-reframework.json")?;
    if target_exists != metadata_exists {
        return Err(Error::Message(
            "unmanaged dinput8.dll already exists; refusing overwrite".into(),
        ));
    }
    let old = if target_exists {
        let bytes = directory.read_regular("dinput8.dll")?;
        let current: Metadata =
            serde_json::from_slice(&directory.read_regular(".fling-reframework.json")?)?;
        let current_digest = digest(&bytes);
        if current.appid != metadata.appid
            || current.installed_file != "dinput8.dll"
            || (current.sha256 != current_digest && current_digest != metadata.sha256)
        {
            return Err(Error::Message(
                "existing dinput8.dll changed outside Fling; refusing overwrite".into(),
            ));
        }
        Some(bytes)
    } else {
        None
    };
    let dll_temp = directory.write_exclusive(".dinput8.dll.fling-", dll)?;
    let mut encoded = serde_json::to_vec_pretty(&metadata)?;
    encoded.push(b'\n');
    let metadata_temp = directory.write_exclusive(".fling-reframework-", &encoded)?;
    let rollback_temp =
        directory.write_exclusive(".dinput8.dll.rollback-", old.as_deref().unwrap_or_default())?;
    let result = (|| -> Result<(), Error> {
        directory.rename(&metadata_temp, ".fling-reframework.pending.json")?;
        directory.sync()?;
        directory.rename(&dll_temp, "dinput8.dll")?;
        if fail_metadata_commit {
            return Err(Error::Message("injected metadata commit failure".into()));
        }
        directory.rename(".fling-reframework.pending.json", ".fling-reframework.json")?;
        directory.sync()?;
        Ok(())
    })();
    if let Err(error) = result {
        if old.is_some() {
            let _ = directory.rename(&rollback_temp, "dinput8.dll");
        } else if directory.exists("dinput8.dll").unwrap_or(false) {
            let _ = directory.unlink("dinput8.dll");
        }
        if directory
            .exists(".fling-reframework.pending.json")
            .unwrap_or(false)
        {
            let _ = directory.unlink(".fling-reframework.pending.json");
        }
        let _ = directory.sync();
        for temporary in [&dll_temp, &metadata_temp, &rollback_temp] {
            if directory.exists(temporary).unwrap_or(false) {
                let _ = directory.unlink(temporary);
            }
        }
        return Err(error);
    }
    if directory.exists(&rollback_temp)? {
        directory.unlink(&rollback_temp)?;
    }
    Ok(())
}

pub fn install(config: &Config, appid: u32) -> Result<(), Error> {
    if appid != APPID {
        return Err(Error::Message(format!(
            "REFramework automation is not enabled for app {appid}"
        )));
    }
    let game = steam::game_dir(config, appid)?;
    let workspace = tempfile::Builder::new()
        .prefix("fling-reframework-")
        .tempdir_in(env::temp_dir())?;
    let archive = workspace.path().join("REFramework.zip");
    curl(URL, &archive)?;
    if fs::metadata(&archive)?.len() > 67_108_864 || !archive::validate(&archive) {
        let _ = fs::remove_file(&archive);
        return Err(Error::Message(
            "REFramework archive is invalid or unsafe".into(),
        ));
    }
    let bytes = fs::read(&archive)?;
    let expected = if env::var("FLING_TESTING").as_deref() == Ok("1") {
        env::var("FLING_REFRAMEWORK_SHA256").unwrap_or_else(|_| SHA.into())
    } else {
        SHA.into()
    };
    if digest(&bytes) != expected {
        let _ = fs::remove_file(&archive);
        return Err(Error::Message(
            "REFramework archive checksum mismatch".into(),
        ));
    }
    let extracted = workspace.path().join("dinput8.dll");
    let script = r#"import sys,zipfile,pathlib
z=zipfile.ZipFile(sys.argv[1]);m=[i for i in z.infolist() if pathlib.PurePosixPath(i.filename.replace('\\','/')).as_posix().lower()=='dinput8.dll']
sys.exit(2) if len(m)!=1 or m[0].is_dir() or m[0].file_size>67108864 else open(sys.argv[2],'wb').write(z.read(m[0]))"#;
    let s = Command::new("python3")
        .args(["-c", script])
        .arg(&archive)
        .arg(&extracted)
        .status()?;
    let _ = fs::remove_file(&archive);
    if !s.success() {
        return Err(Error::Message(
            "archive must contain exactly one root dinput8.dll".into(),
        ));
    }
    let dll = fs::read(&extracted)?;
    let _ = fs::remove_file(&extracted);
    if !dll.starts_with(b"MZ") {
        return Err(Error::Message("dinput8.dll is not a Windows binary".into()));
    }
    let m = Metadata {
        schema_version: 1,
        appid,
        component: "REFramework".into(),
        installed_file: "dinput8.dll".into(),
        source_url: URL.into(),
        archive_sha256: expected,
        sha256: digest(&dll),
        installed_at: now(),
    };
    install_payload(&game, m, &dll, false)
}
pub(crate) fn stage_removal(config: &Config, appid: u32) -> Result<(), Error> {
    if appid != APPID {
        return Ok(());
    }
    let Ok(directory) = steam::game_dir(config, appid) else {
        return Ok(());
    };
    let dll = "dinput8.dll";
    let metadata = ".fling-reframework.json";
    let dll_tomb = ".fling-remove-dinput8.dll";
    let metadata_tomb = ".fling-remove-reframework.json";
    let mut has_dll = directory.exists(dll)?;
    let mut has_metadata = directory.exists(metadata)?;
    if !has_dll && !has_metadata {
        for tombstone in [dll_tomb, metadata_tomb] {
            if directory.exists(tombstone)? {
                directory.read_regular(tombstone)?;
                directory.unlink(tombstone)?;
            }
        }
        directory.sync()?;
        return Ok(());
    }
    if directory.exists(dll_tomb)? || directory.exists(metadata_tomb)? {
        reconcile_removal_directory(&directory, false)?;
        has_dll = directory.exists(dll)?;
        has_metadata = directory.exists(metadata)?;
    }
    if !has_dll || !has_metadata {
        return Err(Error::Message(
            "managed runtime is incomplete; refusing removal".into(),
        ));
    }
    if directory.exists(".fling-reframework.pending.json")? {
        return Err(Error::Message(
            "runtime transaction is pending; retry installation before removal".into(),
        ));
    }
    directory.rename(metadata, metadata_tomb)?;
    if let Err(error) = directory.rename(dll, dll_tomb) {
        let _ = directory.rename(metadata_tomb, metadata);
        return Err(error.into());
    }
    directory.sync()?;
    let validation = (|| -> Result<(), Error> {
        let dll_bytes = directory.read_regular(dll_tomb)?;
        let managed: Metadata = serde_json::from_slice(&directory.read_regular(metadata_tomb)?)?;
        if managed.appid != appid
            || managed.component != "REFramework"
            || managed.installed_file != dll
            || managed.sha256 != digest(&dll_bytes)
        {
            return Err(Error::Message(
                "managed runtime changed outside Fling; refusing removal".into(),
            ));
        }
        Ok(())
    })();
    if let Err(error) = validation {
        if !directory.exists(dll)? {
            directory.rename(dll_tomb, dll)?;
        }
        if !directory.exists(metadata)? {
            directory.rename(metadata_tomb, metadata)?;
        }
        directory.sync()?;
        return Err(error);
    }
    Ok(())
}

pub fn remove(config: &Config, appid: u32) -> Result<(), Error> {
    stage_removal(config, appid)?;
    reconcile_removal(config, appid, true)
}

pub(crate) fn reconcile_removal(config: &Config, appid: u32, committed: bool) -> Result<(), Error> {
    if appid != APPID {
        return Ok(());
    }
    let directory = steam::game_dir(config, appid)?;
    reconcile_removal_directory(&directory, committed)
}

fn reconcile_removal_directory(directory: &Dir, committed: bool) -> Result<(), Error> {
    const FILES: [(&str, &str); 2] = [
        ("dinput8.dll", ".fling-remove-dinput8.dll"),
        (".fling-reframework.json", ".fling-remove-reframework.json"),
    ];
    let states = FILES
        .iter()
        .map(|(live, tombstone)| {
            Ok((
                *live,
                *tombstone,
                directory.exists(live)?,
                directory.exists(tombstone)?,
            ))
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;

    for (live, _, has_live, has_tombstone) in &states {
        if committed && *has_live {
            return Err(Error::Message(format!(
                "committed runtime removal unexpectedly has live {live}"
            )));
        }
        if !committed && *has_live && *has_tombstone {
            return Err(Error::Message(format!(
                "ambiguous runtime removal state for {live}"
            )));
        }
    }

    for (live, tombstone, has_live, has_tombstone) in states {
        if committed && has_tombstone {
            directory.unlink(tombstone)?;
        } else if !committed && !has_live && has_tombstone {
            directory.rename(tombstone, live)?;
        }
    }
    directory.sync()?;
    Ok(())
}

#[cfg(test)]
mod transaction_tests {
    use super::*;

    fn metadata(appid: u32, dll: &[u8]) -> Metadata {
        Metadata {
            schema_version: 1,
            appid,
            component: "REFramework".into(),
            installed_file: "dinput8.dll".into(),
            source_url: URL.into(),
            archive_sha256: "archive".into(),
            sha256: digest(dll),
            installed_at: "now".into(),
        }
    }

    #[test]
    fn metadata_commit_failure_rolls_back_replaced_dll() {
        let temp = tempfile::tempdir().expect("tempdir");
        let old = b"MZ-old";
        fs::write(temp.path().join("dinput8.dll"), old).expect("old dll");
        fs::write(
            temp.path().join(".fling-reframework.json"),
            serde_json::to_vec(&metadata(APPID, old)).expect("metadata"),
        )
        .expect("old metadata");
        let directory = Dir::open_verified(temp.path()).expect("directory");
        let result = install_payload(&directory, metadata(APPID, b"MZ-new"), b"MZ-new", true);
        assert!(result.is_err());
        assert_eq!(
            fs::read(temp.path().join("dinput8.dll")).expect("restored dll"),
            old
        );
        assert!(!temp.path().join(".fling-reframework.pending.json").exists());
    }

    #[test]
    fn matching_pending_metadata_is_recovered_durably() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dll = b"MZ-new";
        fs::write(temp.path().join("dinput8.dll"), dll).expect("dll");
        fs::write(
            temp.path().join(".fling-reframework.pending.json"),
            serde_json::to_vec(&metadata(APPID, dll)).expect("metadata"),
        )
        .expect("pending");
        recover_pending(
            &crate::fs_safe::Dir::open_verified(temp.path()).expect("directory"),
            APPID,
        )
        .expect("recover");
        assert!(temp.path().join(".fling-reframework.json").is_file());
        assert!(!temp.path().join(".fling-reframework.pending.json").exists());
    }

    #[test]
    fn staged_recovery_restores_each_single_runtime_tombstone_state() {
        for (missing, tombstone) in [
            ("dinput8.dll", ".fling-remove-dinput8.dll"),
            (".fling-reframework.json", ".fling-remove-reframework.json"),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            fs::write(temp.path().join("dinput8.dll"), b"dll").expect("dll");
            fs::write(temp.path().join(".fling-reframework.json"), b"metadata").expect("metadata");
            fs::rename(temp.path().join(missing), temp.path().join(tombstone))
                .expect("partial tombstone");
            reconcile_removal_directory(
                &Dir::open_verified(temp.path()).expect("directory"),
                false,
            )
            .expect("restore staged state");
            assert!(temp.path().join("dinput8.dll").is_file());
            assert!(temp.path().join(".fling-reframework.json").is_file());
            assert!(!temp.path().join(tombstone).exists());
        }
    }

    #[test]
    fn stage_removal_retries_each_single_runtime_tombstone_state() {
        for (live, tombstone) in [
            ("dinput8.dll", ".fling-remove-dinput8.dll"),
            (".fling-reframework.json", ".fling-remove-reframework.json"),
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            let steam = temp.path().join("steam");
            let game = steam.join("steamapps/common/Game");
            fs::create_dir_all(&game).expect("game directory");
            fs::write(
                steam.join("steamapps/appmanifest_3357650.acf"),
                r#""appid" "3357650" "name" "Game" "installdir" "Game""#,
            )
            .expect("manifest");
            let dll = b"MZ-runtime";
            fs::write(game.join("dinput8.dll"), dll).expect("dll");
            fs::write(
                game.join(".fling-reframework.json"),
                serde_json::to_vec(&metadata(APPID, dll)).expect("metadata"),
            )
            .expect("metadata file");
            fs::rename(game.join(live), game.join(tombstone)).expect("partial stage");
            let config = Config {
                home: temp.path().join("home"),
                steam_root: steam,
                trainers: temp.path().join("Trainers"),
                proc_root: temp.path().join("proc"),
            };

            stage_removal(&config, APPID).expect("retry partial staging");

            assert!(!game.join("dinput8.dll").exists());
            assert!(!game.join(".fling-reframework.json").exists());
            assert!(game.join(".fling-remove-dinput8.dll").is_file());
            assert!(game.join(".fling-remove-reframework.json").is_file());
        }
    }

    #[test]
    fn committed_recovery_deletes_either_single_runtime_tombstone() {
        for tombstone in [
            ".fling-remove-dinput8.dll",
            ".fling-remove-reframework.json",
        ] {
            let temp = tempfile::tempdir().expect("tempdir");
            fs::write(temp.path().join(tombstone), b"garbage").expect("tombstone");
            reconcile_removal_directory(&Dir::open_verified(temp.path()).expect("directory"), true)
                .expect("finish committed state");
            assert!(!temp.path().join(tombstone).exists());
        }
    }

    #[test]
    fn committed_recovery_validates_all_runtime_state_before_deleting_tombstones() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join(".fling-remove-dinput8.dll"), b"dll").expect("DLL tombstone");
        fs::write(
            temp.path().join(".fling-reframework.json"),
            b"live metadata",
        )
        .expect("live metadata");

        assert!(
            reconcile_removal_directory(
                &Dir::open_verified(temp.path()).expect("directory"),
                true,
            )
            .is_err()
        );
        assert!(temp.path().join(".fling-remove-dinput8.dll").is_file());
        assert!(temp.path().join(".fling-reframework.json").is_file());
    }

    #[test]
    fn staged_recovery_validates_all_runtime_state_before_restoring_tombstones() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join(".fling-remove-dinput8.dll"), b"dll").expect("DLL tombstone");
        fs::write(
            temp.path().join(".fling-reframework.json"),
            b"live metadata",
        )
        .expect("live metadata");
        fs::write(
            temp.path().join(".fling-remove-reframework.json"),
            b"metadata tombstone",
        )
        .expect("metadata tombstone");

        assert!(
            reconcile_removal_directory(
                &Dir::open_verified(temp.path()).expect("directory"),
                false,
            )
            .is_err()
        );
        assert!(!temp.path().join("dinput8.dll").exists());
        assert!(temp.path().join(".fling-remove-dinput8.dll").is_file());
        assert!(temp.path().join(".fling-reframework.json").is_file());
        assert!(temp.path().join(".fling-remove-reframework.json").is_file());
    }

    #[test]
    fn reconciliation_for_other_apps_is_not_applicable_without_steam() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config = Config {
            home: temp.path().join("home"),
            steam_root: temp.path().join("missing-steam"),
            trainers: temp.path().join("Trainers"),
            proc_root: temp.path().join("proc"),
        };
        reconcile_removal(&config, 42, true).expect("not applicable");
    }
}
