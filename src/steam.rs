use crate::{config::Config, error::Error};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
pub struct Game {
    pub appid: u32,
    pub name: String,
    pub install_dir: String,
    pub library_path: String,
    pub trainer_installed: bool,
    pub trainer_path: Option<String>,
    pub running: bool,
}

pub fn vdf_values(text: &str) -> Vec<(String, String)> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut value = String::new();
        let mut end = start + 1;
        while let Some((i, c)) = chars.next() {
            end = i + c.len_utf8();
            if c == '"' {
                break;
            }
            if c == '\\'
                && let Some(&(_, n)) = chars.peek()
                && (n == '"' || n == '\\')
            {
                if let Some((_, escaped)) = chars.next() {
                    value.push(escaped);
                    end += escaped.len_utf8();
                }
                continue;
            }
            value.push(c)
        }
        tokens.push((start, end, value));
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < tokens.len() {
        let between = &text[tokens[i].1..tokens[i + 1].0];
        if between.chars().all(char::is_whitespace) {
            out.push((tokens[i].2.clone(), tokens[i + 1].2.clone()));
            i += 2
        } else {
            i += 1
        }
    }
    out
}

pub fn libraries(config: &Config) -> Vec<PathBuf> {
    let mut libs = vec![config.steam_root.clone()];
    if let Ok(text) = fs::read_to_string(config.steam_root.join("steamapps/libraryfolders.vdf")) {
        for (k, v) in vdf_values(&text) {
            let p = PathBuf::from(v);
            if k == "path" && !libs.contains(&p) {
                libs.push(p)
            }
        }
    }
    libs
}

pub fn games(config: &Config) -> Vec<Game> {
    let mut result = BTreeMap::new();
    for lib in libraries(config) {
        let Ok(entries) = fs::read_dir(lib.join("steamapps")) else {
            continue;
        };
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.starts_with("appmanifest_") && s.ends_with(".acf"))
            })
            .collect();
        paths.sort();
        for path in paths {
            let Ok(text) = fs::read_to_string(path) else {
                continue;
            };
            let fields: HashMap<_, _> = vdf_values(&text).into_iter().collect();
            let Ok(appid) = fields
                .get("appid")
                .map(String::as_str)
                .unwrap_or("")
                .parse()
            else {
                continue;
            };
            let Some(name) = fields.get("name").filter(|s| !s.is_empty()) else {
                continue;
            };
            let trainer = find_trainer(config, appid);
            result.insert(
                appid,
                Game {
                    appid,
                    name: name.clone(),
                    install_dir: fields.get("installdir").cloned().unwrap_or_default(),
                    library_path: lib.to_string_lossy().into(),
                    trainer_installed: trainer.is_some(),
                    trainer_path: trainer.map(|p| p.to_string_lossy().into()),
                    running: false,
                },
            );
        }
    }
    result.into_values().collect()
}

pub fn safe_name(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars() {
        if c == '/' || c == '\\' || c.is_control() {
            s.push('_')
        } else {
            s.push(c)
        }
    }
    while s.len() > 180 {
        s.pop();
    }
    if s.is_empty() { "_".into() } else { s }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainerLookupError {
    Missing,
    Multiple,
    Unsafe,
}

pub fn unique_trainer(config: &Config, appid: u32) -> Result<PathBuf, TrainerLookupError> {
    if fs::symlink_metadata(&config.trainers)
        .ok()
        .is_some_and(|m| m.file_type().is_symlink())
    {
        return Err(TrainerLookupError::Unsafe);
    }
    let entries = fs::read_dir(&config.trainers).map_err(|_| TrainerLookupError::Missing)?;
    let mut found = Vec::new();
    for entry in entries {
        let e = entry.map_err(|_| TrainerLookupError::Unsafe)?;
        let p = e.path();
        if !e
            .file_name()
            .to_string_lossy()
            .starts_with(&format!("{appid} - "))
        {
            continue;
        }
        let m = fs::symlink_metadata(&p).map_err(|_| TrainerLookupError::Unsafe)?;
        let exe = p.join("Trainer.exe");
        let em = fs::symlink_metadata(&exe).map_err(|_| TrainerLookupError::Unsafe)?;
        if m.is_dir() && !m.file_type().is_symlink() && em.is_file() && !em.file_type().is_symlink()
        {
            found.push(exe)
        } else {
            return Err(TrainerLookupError::Unsafe);
        }
    }
    match found.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(TrainerLookupError::Missing),
        _ => Err(TrainerLookupError::Multiple),
    }
}

pub fn find_trainer(config: &Config, appid: u32) -> Option<PathBuf> {
    unique_trainer(config, appid).ok()
}

pub fn game(config: &Config, appid: u32) -> Option<Game> {
    games(config).into_iter().find(|g| g.appid == appid)
}

pub fn game_dir(config: &Config, appid: u32) -> Result<PathBuf, Error> {
    let g = game(config, appid)
        .ok_or_else(|| Error::Message("installed game directory was not found safely".into()))?;
    let rel = Path::new(&g.install_dir);
    if rel.is_absolute()
        || rel
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Message(
            "installed game directory was not found safely".into(),
        ));
    }
    let common = PathBuf::from(g.library_path).join("steamapps/common");
    let common = common
        .canonicalize()
        .map_err(|_| Error::Message("installed game directory was not found safely".into()))?;
    let mut candidate = common.clone();
    for component in rel.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(Error::Message(
                "installed game directory was not found safely".into(),
            ));
        };
        candidate.push(part);
        let metadata = fs::symlink_metadata(&candidate)
            .map_err(|_| Error::Message("installed game directory was not found safely".into()))?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Message(
                "installed game directory was not found safely".into(),
            ));
        }
    }
    let resolved = candidate.canonicalize()?;
    if !resolved.is_dir() || resolved == common || !resolved.starts_with(&common) {
        return Err(Error::Message(
            "installed game directory was not found safely".into(),
        ));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vdf_escaped_quote() {
        assert_eq!(
            vdf_values(r#""name" "Quote \" Quest""#)[0].1,
            "Quote \" Quest"
        );
    }
    #[test]
    fn vdf_skips_section_names() {
        assert_eq!(
            vdf_values("\"AppState\"\n{\n \"appid\" \"10\"\n}"),
            vec![("appid".into(), "10".into())]
        );
    }
    #[test]
    fn safe_names_are_contained() {
        assert_eq!(safe_name("../x\n"), ".._x_");
    }
}
