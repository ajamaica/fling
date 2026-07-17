use std::{
    ffi::{CStr, CString, OsStr, OsString},
    fs, io,
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::ffi::{OsStrExt, OsStringExt},
    },
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn name(value: &OsStr) -> io::Result<CString> {
    if value.as_bytes().contains(&b'/') || value.as_bytes().contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unsafe relative name",
        ));
    }
    CString::new(value.as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in name"))
}

fn mode_is(value: libc::mode_t, kind: libc::mode_t) -> bool {
    value & libc::S_IFMT == kind
}

pub struct Dir {
    fd: OwnedFd,
}

impl Dir {
    pub fn open_verified(path: &Path) -> io::Result<Self> {
        let resolved = path.canonicalize()?;
        let expected = fs::metadata(&resolved)?;
        use std::os::unix::fs::MetadataExt;
        let root = c"/";
        // SAFETY: root is a valid C string and the returned descriptor is owned.
        let root_fd = unsafe {
            libc::open(
                root.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
            )
        };
        if root_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: open returned a fresh descriptor.
        let mut fd = unsafe { OwnedFd::from_raw_fd(root_fd) };
        for component in resolved.components().skip(1) {
            let child = name(component.as_os_str())?;
            // SAFETY: child is a valid C string and fd remains live for the call.
            let next = unsafe {
                libc::openat(
                    fd.as_raw_fd(),
                    child.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if next < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: openat returned a fresh descriptor.
            fd = unsafe { OwnedFd::from_raw_fd(next) };
        }
        // SAFETY: zeroed stat is valid output storage and fd is live.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: stat points to valid storage.
        if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        if stat.st_dev as u64 != expected.dev()
            || stat.st_ino != expected.ino()
            || !mode_is(stat.st_mode, libc::S_IFDIR)
        {
            return Err(io::Error::other("verified directory identity changed"));
        }
        Ok(Self { fd })
    }

    pub fn open_descendant(&self, path: &Path) -> io::Result<Self> {
        let mut fd = None;
        let mut parent = self.fd.as_raw_fd();
        let mut found = false;
        for component in path.components() {
            let std::path::Component::Normal(component) = component else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe descendant path",
                ));
            };
            found = true;
            let child = name(component)?;
            let raw = unsafe {
                libc::openat(
                    parent,
                    child.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let next = unsafe { OwnedFd::from_raw_fd(raw) };
            parent = next.as_raw_fd();
            fd = Some(next);
        }
        if !found {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "descendant path is empty",
            ));
        }
        Ok(Self {
            fd: fd.expect("non-empty traversal has a descriptor"),
        })
    }

    pub fn exists(&self, child: &str) -> io::Result<bool> {
        let child = name(OsStr::new(child))?;
        // SAFETY: output storage and arguments are valid.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                child.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result == 0 {
            Ok(true)
        } else {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(error)
            }
        }
    }

    pub fn is_dir(&self, child: &str) -> io::Result<bool> {
        let child = name(OsStr::new(child))?;
        // SAFETY: output storage and arguments are valid.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                child.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(mode_is(stat.st_mode, libc::S_IFDIR))
    }

    pub fn read_regular(&self, child: &str) -> io::Result<Vec<u8>> {
        let child = name(OsStr::new(child))?;
        // SAFETY: child and fd are valid.
        let raw = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                child.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a fresh descriptor.
        let mut file = unsafe { fs::File::from_raw_fd(raw) };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(io::Error::other("refusing non-regular file"));
        }
        let mut bytes = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    pub fn write_exclusive(&self, prefix: &str, bytes: &[u8]) -> io::Result<String> {
        use std::io::Write;
        for _ in 0..100 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = format!("{prefix}{}-{sequence}", std::process::id());
            let child = name(OsStr::new(&candidate))?;
            // SAFETY: child and fd are valid; mode is provided for O_CREAT.
            let raw = unsafe {
                libc::openat(
                    self.fd.as_raw_fd(),
                    child.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if raw < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error);
            }
            // SAFETY: openat returned a fresh descriptor.
            let mut file = unsafe { fs::File::from_raw_fd(raw) };
            file.write_all(bytes)?;
            file.sync_all()?;
            return Ok(candidate);
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate temporary file",
        ))
    }

    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let from = name(OsStr::new(from))?;
        let to = name(OsStr::new(to))?;
        // SAFETY: both names and the directory descriptor are valid.
        if unsafe {
            libc::renameat(
                self.fd.as_raw_fd(),
                from.as_ptr(),
                self.fd.as_raw_fd(),
                to.as_ptr(),
            )
        } == 0
        {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn unlink(&self, child: &str) -> io::Result<()> {
        self.unlink_flags(child, 0)
    }
    pub fn rmdir(&self, child: &str) -> io::Result<()> {
        self.unlink_flags(child, libc::AT_REMOVEDIR)
    }
    fn unlink_flags(&self, child: &str, flags: i32) -> io::Result<()> {
        let child = name(OsStr::new(child))?;
        // SAFETY: name and descriptor are valid.
        if unsafe { libc::unlinkat(self.fd.as_raw_fd(), child.as_ptr(), flags) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn sync(&self) -> io::Result<()> {
        // SAFETY: descriptor is live.
        if unsafe { libc::fsync(self.fd.as_raw_fd()) } == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub fn remove_tree(&self, child: &str) -> io::Result<()> {
        let child_name = name(OsStr::new(child))?;
        // SAFETY: arguments are valid.
        let raw = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                child_name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a fresh descriptor.
        let child_dir = Self {
            fd: unsafe { OwnedFd::from_raw_fd(raw) },
        };
        // SAFETY: dup creates an independently owned descriptor.
        let duplicate = unsafe { libc::dup(child_dir.fd.as_raw_fd()) };
        if duplicate < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: duplicate is a valid directory descriptor transferred to DIR.
        let stream = unsafe { libc::fdopendir(duplicate) };
        if stream.is_null() {
            // SAFETY: fdopendir did not take ownership on failure.
            unsafe { libc::close(duplicate) };
            return Err(io::Error::last_os_error());
        }
        let mut entries = Vec::<OsString>::new();
        loop {
            // SAFETY: stream remains valid until closed below.
            let entry = unsafe { libc::readdir(stream) };
            if entry.is_null() {
                break;
            }
            // SAFETY: d_name is NUL-terminated for a valid dirent.
            let bytes = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
            if bytes != b"." && bytes != b".." {
                entries.push(OsString::from_vec(bytes.to_vec()));
            }
        }
        // SAFETY: stream was created by fdopendir and is closed exactly once.
        if unsafe { libc::closedir(stream) } != 0 {
            return Err(io::Error::last_os_error());
        }
        for item in entries {
            let item_text = item
                .to_str()
                .ok_or_else(|| io::Error::other("non-UTF8 tree entry"))?;
            if child_dir.is_dir(item_text)? {
                child_dir.remove_tree(item_text)?;
            } else {
                child_dir.unlink(item_text)?;
            }
        }
        drop(child_dir);
        self.rmdir(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opened_directory_is_not_redirected_by_path_replacement() {
        let temp = tempfile::tempdir().expect("tempdir");
        let original = temp.path().join("original");
        let attacker = temp.path().join("attacker");
        fs::create_dir(&original).expect("original");
        fs::create_dir(&attacker).expect("attacker");
        let directory = Dir::open_verified(&original).expect("verified");
        let moved = temp.path().join("moved");
        fs::rename(&original, &moved).expect("move original");
        std::os::unix::fs::symlink(&attacker, &original).expect("replacement");
        let temporary = directory
            .write_exclusive(".safe-", b"content")
            .expect("write");
        assert_eq!(
            fs::read(moved.join(temporary)).expect("original inode"),
            b"content"
        );
        assert!(
            fs::read_dir(attacker)
                .expect("attacker entries")
                .next()
                .is_none()
        );
    }

    #[test]
    fn descendant_traversal_rejects_nested_symlink() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("real/nested")).expect("real path");
        std::os::unix::fs::symlink(temp.path().join("real"), temp.path().join("linked"))
            .expect("symlink");
        let root = Dir::open_verified(temp.path()).expect("root");

        assert!(root.open_descendant(Path::new("linked/nested")).is_err());
        assert!(root.open_descendant(Path::new("real/nested")).is_ok());
    }
}
