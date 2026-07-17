use std::{env, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Config {
    pub home: PathBuf,
    pub steam_root: PathBuf,
    pub trainers: PathBuf,
    pub proc_root: PathBuf,
}

impl Config {
    pub fn load() -> Self {
        let home = PathBuf::from(env::var_os("HOME").unwrap_or_else(|| ".".into()));
        let candidates = [
            env::var_os("FLING_STEAM_ROOT").map(PathBuf::from),
            Some(home.join(".local/share/Steam")),
            Some(home.join(".steam/steam")),
            Some(home.join(".steam/root")),
            Some(home.join(".var/app/com.valvesoftware.Steam/data/Steam")),
        ];
        let steam_root = candidates
            .into_iter()
            .flatten()
            .find(|p| p.join("steamapps/libraryfolders.vdf").is_file())
            .unwrap_or_else(|| home.join(".local/share/Steam"));
        Self {
            trainers: home.join("Trainers"),
            home,
            steam_root,
            proc_root: env::var_os("FLING_PROC_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/proc".into()),
        }
    }
}
