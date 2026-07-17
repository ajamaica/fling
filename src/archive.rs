use crate::error::Error;
use std::{path::Path, process::Command};

// Python's standard library is used as the portable ZIP codec boundary. All policy,
// paths and resource limits are fixed here; no user value is evaluated as code.
const ZIP_HELPER: &str = r#"
import pathlib,stat,sys,zipfile
p=pathlib.Path(sys.argv[1]); mode=sys.argv[2]
try:
 z=zipfile.ZipFile(p); infos=z.infolist()
 if not infos or len(infos)>1024: raise ValueError()
 total=0
 for i in infos:
  n=i.filename; q=pathlib.PurePosixPath(n.replace('\\','/')); m=(i.external_attr>>16)&0xffff
  if not n or '\0' in n or q.is_absolute() or '..' in q.parts or pathlib.PureWindowsPath(n).is_absolute() or stat.S_ISLNK(m) or stat.S_IFMT(m) not in (0,stat.S_IFREG,stat.S_IFDIR): raise ValueError()
  total+=i.file_size
  if i.file_size>268435456 or total>536870912 or (i.file_size and (not i.compress_size or i.file_size/i.compress_size>200)): raise ValueError()
 if mode=='check': sys.exit(0)
 ex=[i for i in infos if not i.is_dir() and i.filename.lower().endswith('.exe')]
 if not ex: print('no executable in trainer archive',file=sys.stderr);sys.exit(3)
 i=max(ex,key=lambda x:x.file_size); data=z.read(i)
 open(sys.argv[3],'wb').write(data)
except (OSError,ValueError,zipfile.BadZipFile): sys.exit(2)
"#;

pub fn validate(path: &Path) -> bool {
    Command::new("python3")
        .args(["-c", ZIP_HELPER])
        .arg(path)
        .arg("check")
        .status()
        .is_ok_and(|s| s.success())
}
pub fn extract_largest_exe(path: &Path, dest: &Path) -> Result<(), Error> {
    let s = Command::new("python3")
        .args(["-c", ZIP_HELPER])
        .arg(path)
        .arg("extract")
        .arg(dest)
        .status()?;
    if s.success() {
        Ok(())
    } else if s.code() == Some(3) {
        Err(Error::Message("no executable in trainer archive".into()))
    } else {
        Err(Error::Message(
            "trainer archive is invalid or unsafe".into(),
        ))
    }
}
