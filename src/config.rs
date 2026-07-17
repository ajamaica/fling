use std::{env, ffi::CStr, io, os::unix::ffi::OsStringExt, path::PathBuf};

use crate::error::Error;

#[derive(Clone, Debug)]
pub struct Config {
    pub home: PathBuf,
    pub steam_root: PathBuf,
    pub trainers: PathBuf,
    pub proc_root: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self, Error> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map(Ok)
            .unwrap_or_else(account_home)?;
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
        Ok(Self {
            trainers: home.join("Trainers"),
            home,
            steam_root,
            proc_root: env::var_os("FLING_PROC_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| "/proc".into()),
        })
    }
}

fn account_home() -> Result<PathBuf, Error> {
    let buffer_size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    let mut buffer = vec![0_u8; usize::try_from(buffer_size).unwrap_or(16_384).max(1024)];
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    let status = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 {
        return Err(Error::Io(io::Error::from_raw_os_error(status)));
    }
    if result.is_null() {
        return Err(Error::Message(
            "current account home directory was not found".into(),
        ));
    }
    let record = unsafe { record.assume_init() };
    home_from_pw_dir(record.pw_dir)
}

fn home_from_pw_dir(pw_dir: *const libc::c_char) -> Result<PathBuf, Error> {
    if pw_dir.is_null() {
        return Err(Error::Message(
            "current account home directory was not found".into(),
        ));
    }
    let bytes = unsafe { CStr::from_ptr(pw_dir) }.to_bytes().to_vec();
    let home = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if !home.is_absolute() {
        return Err(Error::Message(
            "current account home directory is not absolute".into(),
        ));
    }
    Ok(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_account_home_pointer_is_rejected() {
        assert!(home_from_pw_dir(std::ptr::null()).is_err());
    }
}
