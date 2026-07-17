use fling_cli::{
    config::Config,
    steam::{TrainerLookupError, unique_trainer},
};
use std::fs;

fn config() -> (tempfile::TempDir, Config) {
    let temp = tempfile::tempdir().expect("tempdir");
    let trainers = temp.path().join("Trainers");
    fs::create_dir(&trainers).expect("trainers");
    let config = Config {
        home: temp.path().join("home"),
        steam_root: temp.path().join("steam"),
        proc_root: temp.path().join("proc"),
        trainers,
    };
    (temp, config)
}

#[test]
fn explicitly_refuses_multiple_valid_trainer_directories() {
    let (_temp, config) = config();
    for name in ["42 - Old", "42 - New"] {
        fs::create_dir(config.trainers.join(name)).expect("trainer dir");
        fs::write(config.trainers.join(name).join("Trainer.exe"), b"MZ").expect("trainer");
    }
    assert_eq!(
        unique_trainer(&config, 42),
        Err(TrainerLookupError::Multiple)
    );
}

#[test]
fn distinguishes_missing_from_unsafe_candidates() {
    let (_temp, config) = config();
    assert_eq!(
        unique_trainer(&config, 42),
        Err(TrainerLookupError::Missing)
    );
    std::os::unix::fs::symlink("/tmp", config.trainers.join("42 - Link")).expect("symlink");
    assert_eq!(unique_trainer(&config, 42), Err(TrainerLookupError::Unsafe));
}
