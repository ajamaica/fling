#!/usr/bin/env python3
import hashlib
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
BOOTSTRAP = ROOT / "install.sh"
BUNDLE_INSTALL = ROOT / "packaging/install-bundle.sh"
PACKAGE = ROOT / "packaging/package-release.sh"


class ReleaseInstallerTest(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="fling-release-test-"))

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def make_release(self):
        release = self.tmp / "release" / "fling-linux-x86_64"
        release.mkdir(parents=True, exist_ok=True)
        installer = release / "install-bundle.sh"
        installer.write_text("#!/usr/bin/env bash\nprintf installed > \"$HOME/installed\"\n")
        installer.chmod(0o755)
        archive = self.tmp / "fling-linux-x86_64.tar.gz"
        with tarfile.open(archive, "w:gz") as output:
            output.add(release, arcname="fling-linux-x86_64")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        sums = self.tmp / "SHA256SUMS"
        sums.write_text(f"{digest}  {archive.name}\n")
        return archive, sums

    def fake_curl(self, archive, sums):
        fakebin = self.tmp / "fakebin"
        fakebin.mkdir(exist_ok=True)
        curl = fakebin / "curl"
        curl.write_text("""#!/usr/bin/env bash
set -eu
printf '%s\\n' "$*" >> "$CURL_LOG"
url="${!#}"
out=""
while [ "$#" -gt 0 ]; do
  [ "$1" != "-o" ] || { out="$2"; break; }
  shift
done
case "$url" in
  */SHA256SUMS) cp "$FAKE_SUMS" "$out" ;;
  */fling-linux-x86_64.tar.gz) cp "$FAKE_ARCHIVE" "$out" ;;
  *) exit 22 ;;
esac
""")
        curl.chmod(0o755)
        uname = fakebin / "uname"
        uname.write_text("""#!/usr/bin/env bash
case "${1:-}" in -s) echo Linux ;; -m) echo x86_64 ;; *) echo Linux ;; esac
""")
        uname.chmod(0o755)
        return fakebin

    def run_bootstrap(self, version=None, sums_text=None):
        archive, sums = self.make_release()
        if sums_text is not None:
            sums.write_text(sums_text)
        fakebin = self.fake_curl(archive, sums)
        home = self.tmp / "home"
        home.mkdir(exist_ok=True)
        log = self.tmp / "curl.log"
        log.unlink(missing_ok=True)
        checksum_bin = pathlib.Path(shutil.which("sha256sum")).parent
        env = os.environ | {
            "HOME": str(home), "PATH": f"{fakebin}:{checksum_bin}:/usr/bin:/bin",
            "CURL_LOG": str(log), "FAKE_ARCHIVE": str(archive),
            "FAKE_SUMS": str(sums),
        }
        if version is not None:
            env["FLING_VERSION"] = version
        result = subprocess.run(["/bin/bash", BOOTSTRAP], env=env, text=True,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return result, home, log.read_text() if log.exists() else ""

    def test_bootstrap_latest_downloads_only_fixed_github_release_urls(self):
        result, home, log = self.run_bootstrap()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((home / "installed").is_file())
        base = "https://github.com/ajamaica/fling/releases/latest/download/"
        self.assertIn(base + "fling-linux-x86_64.tar.gz", log)
        self.assertIn(base + "SHA256SUMS", log)

    def test_bootstrap_uses_validated_configured_release_tag(self):
        result, _, log = self.run_bootstrap("v1.2.3")
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertIn("/releases/download/v1.2.3/", log)

        rejected, _, rejected_log = self.run_bootstrap("../../latest")
        self.assertNotEqual(0, rejected.returncode)
        self.assertEqual("", rejected_log)

    def test_bootstrap_refuses_checksum_mismatch_before_installing(self):
        bad = "0" * 64 + "  fling-linux-x86_64.tar.gz\n"
        result, home, _ = self.run_bootstrap(sums_text=bad)
        self.assertNotEqual(0, result.returncode)
        self.assertFalse((home / "installed").exists())

    def test_bundle_installer_contract_uses_cli_and_hardened_ui_installers(self):
        text = BUNDLE_INSTALL.read_text()
        self.assertIn('install-cli-from-source.sh', text)
        self.assertIn('install-ui.sh', text)
        self.assertIn('ui-export', text)
        self.assertNotIn('sudo', text)

    def test_bundle_installer_installs_cli_service_and_prebuilt_ui(self):
        bundle = self.tmp / "fling-linux-x86_64"
        for directory in ("bin", "systemd", "packaging", "ui-export"):
            (bundle / directory).mkdir(parents=True)
        for source, relative in (
            (ROOT / "bin/fling", "bin/fling"),
            (ROOT / "systemd/fling-watch.service", "systemd/fling-watch.service"),
            (BUNDLE_INSTALL, "install-bundle.sh"),
            (ROOT / "packaging/install-cli-from-source.sh", "packaging/install-cli-from-source.sh"),
            (ROOT / "packaging/install-ui.sh", "packaging/install-ui.sh"),
        ):
            shutil.copy2(source, bundle / relative)
        ui = bundle / "ui-export/fling-ui.x86_64"
        ui.write_text("prebuilt ui")
        ui.chmod(0o755)
        (bundle / "ui-export/fling-ui.pck").write_text("data")

        fakebin = self.tmp / "commands"
        fakebin.mkdir()
        for command in ("curl", "jq", "file", "busctl", "systemctl", "protontricks-launch"):
            path = fakebin / command
            path.write_text("#!/usr/bin/env bash\nexit 0\n")
            path.chmod(0o755)
        home = self.tmp / "bundle-home"
        home.mkdir()
        python_bin = pathlib.Path(shutil.which("python3")).parent
        result = subprocess.run(["/bin/bash", bundle / "install-bundle.sh"],
                                env=os.environ | {"HOME": str(home), "PATH": f"{fakebin}:{python_bin}:/usr/bin:/bin"},
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((home / ".local/bin/fling").is_file())
        self.assertTrue((home / ".config/systemd/user/fling-watch.service").is_file())
        self.assertEqual("prebuilt ui", (home / ".local/share/fling-ui/fling-ui.x86_64").read_text())
        self.assertTrue((home / ".local/bin/fling-ui").is_file())

    def test_packager_creates_complete_deterministic_release(self):
        export = self.tmp / "export"
        export.mkdir()
        (export / "fling-ui.x86_64").write_text("ui")
        (export / "fling-ui.x86_64").chmod(0o755)
        (export / "fling-ui.pck").write_text("pck")
        first = self.tmp / "first"
        second = self.tmp / "second"
        first.mkdir(); second.mkdir()
        env = os.environ | {"SOURCE_DATE_EPOCH": "1700000000"}
        for destination in (first, second):
            result = subprocess.run(["/bin/bash", PACKAGE, export, destination],
                                    env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            self.assertEqual(0, result.returncode, result.stderr)
        archive = first / "fling-linux-x86_64.tar.gz"
        self.assertEqual(archive.read_bytes(), (second / archive.name).read_bytes())
        self.assertIn(archive.name, (first / "SHA256SUMS").read_text())
        with tarfile.open(archive) as bundle:
            names = set(bundle.getnames())
        for required in (
            "fling-linux-x86_64/install-bundle.sh",
            "fling-linux-x86_64/packaging/install-cli-from-source.sh",
            "fling-linux-x86_64/packaging/install-ui.sh",
            "fling-linux-x86_64/bin/fling",
            "fling-linux-x86_64/systemd/fling-watch.service",
            "fling-linux-x86_64/ui-export/fling-ui.x86_64",
        ):
            self.assertIn(required, names)


if __name__ == "__main__":
    unittest.main(verbosity=2)
