#!/usr/bin/env python3
"""Covers protontricks detection and the Flatpak overrides the CLI installer writes."""
import os, pathlib, shutil, subprocess, tempfile, unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALL = ROOT / "packaging/install-cli-from-source.sh"
PT_ID = "com.github.Matoking.protontricks"

# Commands install-cli-from-source.sh requires but whose real behaviour the test
# does not need; each one just succeeds.
STUBBED = ("curl", "jq", "file", "busctl", "systemctl", "xdotool", "xprop")
# Real tools the installer genuinely uses. PATH is limited to the stub directory
# so a protontricks-launch installed on the test machine cannot leak in.
PASSTHROUGH = ("dirname", "mkdir", "install", "cat", "python3")


class InstallCliTest(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="fling-install-cli-test-"))
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.bin = self.tmp / "stub-bin"
        self.bin.mkdir()
        self.flatpak_log = self.tmp / "flatpak.log"
        self.cli_binary = self.tmp / "fling-rs"
        self.cli_binary.write_text("#!/bin/sh\nexit 0\n")
        self.cli_binary.chmod(0o755)
        for name in STUBBED:
            self.stub(name, "exit 0")
        for name in PASSTHROUGH:
            real = shutil.which(name)
            self.assertIsNotNone(real, f"{name} is required to run this test")
            (self.bin / name).symlink_to(real)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/sh\n" + body + "\n")
        path.chmod(0o755)
        return path

    def stub_flatpak(self, installed):
        # `flatpak info <id>` decides whether the installer treats protontricks as
        # a Flatpak; every `flatpak override` call is recorded for assertions.
        self.stub("flatpak", f"""
case "$1" in
  info) exit {0 if installed else 1} ;;
  override) printf '%s\\n' "$*" >> "{self.flatpak_log}" ;;
esac
exit 0
""".strip())

    def run_install(self):
        env = os.environ | {
            "HOME": str(self.home),
            "PATH": str(self.bin),
            "FLING_CLI_BINARY": str(self.cli_binary),
        }
        return subprocess.run(["/bin/bash", INSTALL], env=env, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def overrides(self):
        if not self.flatpak_log.exists():
            return []
        return self.flatpak_log.read_text().splitlines()

    def test_flatpak_protontricks_is_detected_behind_a_launcher_on_path(self):
        # Some distros ship /usr/bin/protontricks-launch as a `flatpak run` wrapper,
        # so the launcher being on PATH must not be read as a native install.
        self.stub_flatpak(installed=True)
        self.stub("protontricks-launch", "exit 0")

        p = self.run_install()

        self.assertEqual(0, p.returncode, p.stderr)
        self.assertIn("protontricks: Flatpak", p.stdout)
        self.assertTrue(any(PT_ID in line for line in self.overrides()),
                        "expected Flatpak overrides to be applied")

    def test_flatpak_overrides_expose_the_paths_protontricks_launch_is_given(self):
        # protontricks-launch runs inside the sandbox, so it can only open the
        # staged WeMod installer and the shared WeMod install if both are shared in.
        self.stub_flatpak(installed=True)

        p = self.run_install()

        self.assertEqual(0, p.returncode, p.stderr)
        shared = self.overrides()
        for expected in (f"{self.home}/.cache/fling", f"{self.home}/.local/share/fling"):
            with self.subTest(path=expected):
                self.assertTrue(
                    any(f"--filesystem={expected}" in line for line in shared),
                    f"{expected} was not shared with the protontricks Flatpak: {shared}")

    def test_native_protontricks_without_the_flatpak_applies_no_overrides(self):
        self.stub_flatpak(installed=False)
        self.stub("protontricks-launch", "exit 0")

        p = self.run_install()

        self.assertEqual(0, p.returncode, p.stderr)
        self.assertIn("protontricks: native", p.stdout)
        self.assertEqual([], self.overrides())

    def test_missing_protontricks_fails_the_install(self):
        self.stub_flatpak(installed=False)

        p = self.run_install()

        self.assertNotEqual(0, p.returncode)
        self.assertIn("protontricks is required", p.stderr)


if __name__ == "__main__":
    unittest.main()
