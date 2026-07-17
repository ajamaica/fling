use fling_cli::archive;
use std::{fs, process::Command};

fn zip(path: &std::path::Path, member: &str) {
    let status = Command::new("python3").args(["-c",
        "import sys,zipfile; z=zipfile.ZipFile(sys.argv[1],'w'); z.writestr(sys.argv[2],b'MZpayload'); z.close()",
    ]).arg(path).arg(member).status().expect("python");
    assert!(status.success());
}

#[test]
fn archive_policy_accepts_regular_payload_and_rejects_traversal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let valid = temp.path().join("valid.zip");
    let unsafe_zip = temp.path().join("unsafe.zip");
    zip(&valid, "nested/Trainer.exe");
    zip(&unsafe_zip, "../escape.exe");
    assert!(archive::validate(&valid));
    assert!(!archive::validate(&unsafe_zip));
    let extracted = temp.path().join("Trainer.exe");
    archive::extract_largest_exe(&valid, &extracted).expect("extract");
    assert_eq!(fs::read(extracted).expect("payload"), b"MZpayload");
}
