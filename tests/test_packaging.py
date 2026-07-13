#!/usr/bin/env python3
import os, pathlib, shutil, subprocess, tempfile, unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
INSTALL = ROOT / "packaging/install-ui.sh"
UNINSTALL = ROOT / "packaging/uninstall-ui.sh"

class PackagingTest(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="fling-package-test-"))
        self.home = self.tmp / "home"; self.home.mkdir()
        self.source = self.tmp / "export"; self.source.mkdir()
        (self.source / "fling-ui").write_text("binary")
    def tearDown(self): shutil.rmtree(self.tmp)
    def run_install(self):
        return subprocess.run(["/bin/bash", INSTALL, self.source],
                              env=os.environ | {"HOME": str(self.home)}, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    def run_uninstall(self):
        return subprocess.run(["/bin/bash", UNINSTALL],
                              env=os.environ | {"HOME": str(self.home)}, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    def reset_local(self):
        local = self.home / ".local"
        if local.is_symlink():
            local.unlink()
        else:
            shutil.rmtree(local, ignore_errors=True)

    def test_installer_rejects_symlinked_ancestors_without_touching_targets(self):
        paths = (".local", ".local/share", ".local/bin")
        for relative in paths:
            with self.subTest(path=relative):
                self.reset_local()
                target = self.tmp / (relative.replace("/", "-") + "-install-target")
                target.mkdir()
                marker = target / "keep"
                marker.write_text("external")
                link = self.home / relative
                link.parent.mkdir(parents=True, exist_ok=True)
                link.symlink_to(target, target_is_directory=True)

                p = self.run_install()

                self.assertNotEqual(0, p.returncode)
                self.assertEqual("external", marker.read_text())

    def test_uninstaller_rejects_symlinked_ancestors_without_touching_targets(self):
        paths = (".local", ".local/share", ".local/bin")
        for relative in paths:
            with self.subTest(path=relative):
                self.reset_local()
                target = self.tmp / (relative.replace("/", "-") + "-uninstall-target")
                target.mkdir()
                marker = target / "keep"
                marker.write_text("external")
                link = self.home / relative
                link.parent.mkdir(parents=True, exist_ok=True)
                link.symlink_to(target, target_is_directory=True)

                p = self.run_uninstall()

                self.assertNotEqual(0, p.returncode)
                self.assertEqual("external", marker.read_text())

    def test_symlinked_home_itself_is_allowed(self):
        real_home = self.tmp / "real-home"
        real_home.mkdir()
        linked_home = self.tmp / "linked-home"
        linked_home.symlink_to(real_home, target_is_directory=True)

        p = subprocess.run(["/bin/bash", INSTALL, self.source],
                           env=os.environ | {"HOME": str(linked_home)}, text=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        self.assertEqual(0, p.returncode, p.stderr)
        self.assertTrue((real_home / ".local/bin/fling-ui").is_file())

    def test_uninstall_is_idempotent_and_preserves_trainers(self):
        installed = self.run_install()
        trainers = self.home / "Trainers"
        trainers.mkdir()
        marker = trainers / "keep"
        marker.write_text("trainer")

        first = self.run_uninstall()
        second = self.run_uninstall()

        self.assertEqual(0, installed.returncode, installed.stderr)
        self.assertEqual(0, first.returncode, first.stderr)
        self.assertEqual(0, second.returncode, second.stderr)
        self.assertEqual("trainer", marker.read_text())
        self.assertFalse((self.home / ".local/share/fling-ui").exists())

    def test_rejects_symlinked_install_roots_without_touching_targets(self):
        paths = (".local/share/fling-ui", ".local/bin", ".local/share/applications")
        for relative in paths:
            with self.subTest(path=relative):
                shutil.rmtree(self.home / ".local", ignore_errors=True)
                target = self.tmp / (relative.replace("/", "-") + "-target")
                target.mkdir(); marker = target / "keep"; marker.write_text("safe")
                link = self.home / relative; link.parent.mkdir(parents=True, exist_ok=True)
                link.symlink_to(target, target_is_directory=True)
                p = subprocess.run(["/bin/bash", INSTALL, self.source], env=os.environ | {"HOME": str(self.home)},
                                   text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                self.assertNotEqual(0, p.returncode)
                self.assertTrue(marker.is_file())

    def test_missing_export_error_lists_all_supported_names(self):
        (self.source / "fling-ui").unlink()

        result = self.run_install()

        self.assertNotEqual(0, result.returncode)
        self.assertIn("FlingUi.x86_64", result.stderr)
        self.assertIn("Fling UI.x86_64", result.stderr)

    def test_solution_maps_release_configuration_to_release(self):
        solution = (ROOT / "ui/FlingUi.sln").read_text(encoding="utf-8-sig")

        self.assertIn("Release|Any CPU.ActiveCfg = Release|Any CPU", solution)
        self.assertIn("Release|Any CPU.Build.0 = Release|Any CPU", solution)

    def test_readme_documents_linux_export_installation(self):
        readme = (ROOT / "README.md").read_text()

        self.assertIn("fling-ui.x86_64", readme)
        self.assertIn("./packaging/install-ui.sh /path/to/linux-export-directory", readme)
        self.assertIn("dotnet build ui/FlingUi.sln", readme)
        self.assertIn("Add a Non-Steam Game", readme)

    def test_installs_standard_godot_linux_x86_64_export(self):
        (self.source / "fling-ui").unlink()
        (self.source / "fling-ui.x86_64").write_text("godot binary")

        result = self.run_install()

        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue((self.home / ".local/share/fling-ui/fling-ui.x86_64").is_file())
        launcher = (self.home / ".local/bin/fling-ui").read_text()
        self.assertIn("fling-ui.x86_64", launcher)

    def test_rejects_symlinked_export_executable_without_touching_target(self):
        target = self.tmp / "external-executable"
        target.write_text("external")
        (self.source / "fling-ui").unlink()
        (self.source / "fling-ui").symlink_to(target)

        p = self.run_install()

        self.assertNotEqual(0, p.returncode)
        self.assertEqual("external", target.read_text())

    def test_rejects_symlinked_export_root_without_touching_target(self):
        target = self.tmp / "external-export"
        target.mkdir()
        marker = target / "fling-ui"
        marker.write_text("external")
        linked_source = self.tmp / "linked-export"
        linked_source.symlink_to(target, target_is_directory=True)

        p = subprocess.run(["/bin/bash", INSTALL, linked_source],
                           env=os.environ | {"HOME": str(self.home)}, text=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)

        self.assertNotEqual(0, p.returncode)
        self.assertEqual("external", marker.read_text())

    def test_rejects_nested_export_symlink_without_touching_target(self):
        target = self.tmp / "external-data"
        target.write_text("external")
        nested = self.source / "data"
        nested.mkdir()
        (nested / "linked-data").symlink_to(target)

        p = self.run_install()

        self.assertNotEqual(0, p.returncode)
        self.assertEqual("external", target.read_text())

    def test_rejects_existing_launcher_symlink_without_touching_target(self):
        target = self.tmp / "external-launcher"
        target.write_text("external")
        launcher = self.home / ".local/bin/fling-ui"
        launcher.parent.mkdir(parents=True)
        launcher.symlink_to(target)

        p = self.run_install()

        self.assertNotEqual(0, p.returncode)
        self.assertEqual("external", target.read_text())

    def test_rejects_existing_desktop_symlink_without_touching_target(self):
        target = self.tmp / "external-desktop"
        target.write_text("external")
        desktop = self.home / ".local/share/applications/fling-ui.desktop"
        desktop.parent.mkdir(parents=True)
        desktop.symlink_to(target)

        p = self.run_install()

        self.assertNotEqual(0, p.returncode)
        self.assertEqual("external", target.read_text())

    def test_replaces_regular_install_files_idempotently(self):
        launcher = self.home / ".local/bin/fling-ui"
        desktop = self.home / ".local/share/applications/fling-ui.desktop"
        launcher.parent.mkdir(parents=True)
        desktop.parent.mkdir(parents=True)
        launcher.write_text("old launcher")
        desktop.write_text("old desktop")

        first = self.run_install()
        second = self.run_install()

        self.assertEqual(0, first.returncode, first.stderr)
        self.assertEqual(0, second.returncode, second.stderr)
        self.assertIn("exec ", launcher.read_text())
        self.assertIn("[Desktop Entry]", desktop.read_text())

if __name__ == "__main__": unittest.main(verbosity=2)
