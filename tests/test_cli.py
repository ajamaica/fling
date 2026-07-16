#!/usr/bin/env python3
"""Offline integration tests for fling's stable JSON API."""
import json
import hashlib
import os
import pathlib
import shutil
import stat
import subprocess
import tempfile
import unittest
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
FLING = ROOT / "bin" / "fling"


class FlingCliTest(unittest.TestCase):
    def setUp(self):
        self.tmp = pathlib.Path(tempfile.mkdtemp(prefix="fling-test-"))
        self.home = self.tmp / "home"
        self.steam = self.home / "Steam Root"
        self.bin = self.tmp / "bin"
        self.home.mkdir(); self.bin.mkdir()
        (self.steam / "steamapps").mkdir(parents=True)
        self.lib2 = self.tmp / "Library With Spaces"
        (self.lib2 / "steamapps").mkdir(parents=True)
        (self.steam / "steamapps/libraryfolders.vdf").write_text(
            f'"libraryfolders"\n{{\n "0" {{ "path" "{self.steam}" }}\n'
            f' "1" {{ "path" "{self.lib2}" }}\n}}\n'
        )
        self.manifest(self.steam, "10", 'Quote " Quest', "Quote Quest")
        self.manifest(self.lib2, "20", "Space Game", "Space Game")
        (self.lib2 / "steamapps/appmanifest_bad.acf").write_text('"AppState" { "name" "broken" }')
        self.env = os.environ | {
            "HOME": str(self.home), "FLING_STEAM_ROOT": str(self.steam),
            "PATH": f"{self.bin}:{os.environ['PATH']}", "LC_ALL": "C",
        }

    def command(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/bash\nset -eu\n" + body)
        path.chmod(path.stat().st_mode | stat.S_IXUSR)

    def mock_download(self, detected="PE32 executable", page_code="200", link=True,
                      payload_path=None):
        curl_log = self.tmp / "curl.log"
        href = 'https://flingtrainer.com/downloads/mock.bin' if link else ''
        payload_command = (f'cp "{payload_path}" "$out"' if payload_path
                           else "printf 'mock-payload' > \"$out\"")
        self.command("curl", f'''printf '%s\\n' "$*" >> "{curl_log}"
out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
case "$*" in
  *%\\{{http_code\\}}*) printf '{page_code}' ;;
  *downloads/mock.bin*) {payload_command} ;;
  *) printf '<a href="{href}">trainer</a>' ;;
esac
''')
        self.command("file", f"printf '%s\\n' '{detected}'\n")
        return curl_log

    def enable_legacy_pcre_grep(self):
        # The legacy commands use GNU grep's -P option, which is absent on macOS.
        self.command("grep", r'''if [ "${1:-}" = -oP ]; then
  pattern="$2"; shift 2
  PATTERN="$pattern" exec perl -ne 'while (/$ENV{PATTERN}/g) { print "$&\n" }' "$@"
fi
exec /usr/bin/grep "$@"
''')

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def manifest(self, lib, appid, name, installdir):
        escaped_name = name.replace('"', '\\"')
        (lib / f"steamapps/appmanifest_{appid}.acf").write_text(
            f'"AppState"\n{{\n "appid" "{appid}"\n "name" "{escaped_name}"\n'
            f' "installdir" "{installdir}"\n}}\n'
        )

    def invoke(self, *args, check=False):
        p = subprocess.run(["/bin/bash", FLING, *args], env=self.env, text=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        if check and p.returncode:
            self.fail(f"rc={p.returncode}\nstdout={p.stdout}\nstderr={p.stderr}")
        return p

    def payload(self, p):
        try: return json.loads(p.stdout)
        except Exception: self.fail(f"stdout is not JSON: {p.stdout!r}; stderr={p.stderr!r}")

    def patch_zip_sizes(self, path, uncompressed_size, compressed_size=None):
        data = bytearray(path.read_bytes())
        compressed_size = uncompressed_size if compressed_size is None else compressed_size
        for signature, compressed_offset, uncompressed_offset in ((b"PK\x03\x04", 18, 22),
                                                                  (b"PK\x01\x02", 20, 24)):
            start = 0
            while True:
                start = data.find(signature, start)
                if start < 0: break
                data[start + compressed_offset:start + compressed_offset + 4] = compressed_size.to_bytes(4, "little")
                data[start + uncompressed_offset:start + uncompressed_offset + 4] = uncompressed_size.to_bytes(4, "little")
                start += 4
        path.write_bytes(data)

    def allow_test_reframework_archive(self, archive):
        self.env["FLING_TESTING"] = "1"
        self.env["FLING_REFRAMEWORK_SHA256"] = hashlib.sha256(archive.read_bytes()).hexdigest()

    def test_games_handles_spaces_escaping_malformed_and_installed(self):
        trainer = self.home / 'Trainers/10 - Quote " Quest/Trainer.exe'
        trainer.parent.mkdir(parents=True); trainer.write_bytes(b"MZ valid")
        p = self.invoke("games", "--json", check=True); data = self.payload(p)
        self.assertEqual(1, data["schema_version"])
        self.assertEqual([10, 20], [g["appid"] for g in data["games"]])
        self.assertEqual('Quote " Quest', data["games"][0]["name"])
        self.assertTrue(data["games"][0]["trainer_installed"])
        self.assertIn("Library With Spaces", data["games"][1]["library_path"])
        self.assertNotIn("\x1b", p.stdout)

    def test_installed_uses_same_shape_and_ignores_non_regular_trainer(self):
        good = self.home / "Trainers/20 - Space Game/Trainer.exe"
        good.parent.mkdir(parents=True); good.write_bytes(b"MZ")
        bad = self.home / "Trainers/10 - Quote Quest/Trainer.exe"
        bad.parent.mkdir(parents=True); bad.symlink_to(good)
        data = self.payload(self.invoke("installed", "--json", check=True))
        self.assertEqual([20], [g["appid"] for g in data["games"]])

    def test_status_has_stable_fields(self):
        data = self.payload(self.invoke("status", "--json", check=True))
        expected = {"schema_version", "cli_installed", "watcher_installed", "watcher_active",
                    "global_environment_configured", "steam_environment_active", "steam_running",
                    "steam_root", "trainers_directory"}
        self.assertEqual(expected, set(data))

    def test_game_ready_waits_for_installed_game_process_after_secondary_launcher(self):
        proc = self.tmp / "proc"
        launcher = proc / "101"
        launcher.mkdir(parents=True)
        (launcher / "environ").write_bytes(b"STEAM_COMPAT_APP_ID=20\0WINEPREFIX=/prefix\0")
        (launcher / "cmdline").write_bytes(
            b"C:\\Program Files\\Electronic Arts\\EA Desktop\\EADesktop.exe\0-silent\0"
        )
        env = self.env | {"FLING_PROC_ROOT": str(proc)}

        waiting = subprocess.run(["/bin/bash", FLING, "_game-ready", "20"], env=env,
                                 text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(1, waiting.returncode, waiting.stdout + waiting.stderr)

        installer = proc / "102"
        installer.mkdir()
        (installer / "environ").write_bytes(
            b"STEAM_COMPAT_APP_ID=20\0WINEPREFIX=/prefix\0"
        )
        (installer / "cmdline").write_bytes(
            b"S:\\common\\Space Game\\_CommonRedist\\EAappInstaller.exe\0"
        )
        still_waiting = subprocess.run(
            ["/bin/bash", FLING, "_game-ready", "20"], env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(
            1, still_waiting.returncode, still_waiting.stdout + still_waiting.stderr
        )

        helper = proc / "104"
        helper.mkdir()
        (helper / "environ").write_bytes(
            b"STEAM_COMPAT_APP_ID=20\0WINEPREFIX=/prefix\0"
        )
        (helper / "cmdline").write_bytes(
            b"S:\\common\\Space Game\\__Installer\\cleanup.exe\0"
        )
        helper_waiting = subprocess.run(
            ["/bin/bash", FLING, "_game-ready", "20"], env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(
            1, helper_waiting.returncode,
            helper_waiting.stdout + helper_waiting.stderr,
        )

        wrapper = proc / "103"
        wrapper.mkdir()
        (wrapper / "environ").write_bytes(
            b"STEAM_COMPAT_APP_ID=20\0WINEPREFIX=/prefix\0"
        )
        (wrapper / "cmdline").write_bytes(
            b"python3\0/proton/wrapper.py\0"
            b"S:\\common\\Space Game\\Binaries\\Win64\\SpaceGame.exe\0"
        )
        wrapper_waiting = subprocess.run(
            ["/bin/bash", FLING, "_game-ready", "20"], env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(
            1, wrapper_waiting.returncode,
            wrapper_waiting.stdout + wrapper_waiting.stderr,
        )

        game = proc / "202"
        game.mkdir()
        (game / "environ").write_bytes(b"STEAM_COMPAT_APP_ID=20\0WINEPREFIX=/prefix\0")
        (game / "cmdline").write_bytes(
            b"S:\\common\\Space Game\\Binaries\\Win64\\SpaceGame.exe\0"
        )
        ready = subprocess.run(["/bin/bash", FLING, "_game-ready", "20"], env=env,
                               text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.assertEqual(0, ready.returncode, ready.stdout + ready.stderr)

    def test_game_ready_reports_unavailable_detection_separately(self):
        proc = self.tmp / "empty-proc"
        proc.mkdir()
        env = self.env | {"FLING_PROC_ROOT": str(proc)}
        unavailable = subprocess.run(
            ["/bin/bash", FLING, "_game-ready", "999"], env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(
            2, unavailable.returncode,
            unavailable.stdout + unavailable.stderr,
        )

    def test_watcher_uses_process_readiness_instead_of_a_global_delay(self):
        source = FLING.read_text()
        watch = source[source.index("cmd_watch() {"):source.index("cmd_installed() {")]
        self.assertNotIn("sleep 10", watch)
        self.assertIn('game_process_ready "$svc"', watch)
        self.assertLess(watch.index('game_process_ready "$svc"'),
                        watch.index('seen[$key]=in-flight'))
        self.assertIn('active[$key]=1', watch)
        self.assertIn("unset 'seen[$key]'", watch)
        self.assertIn("unset 'waiting[$key]'", watch)

    def test_watcher_retries_a_failed_direct_launch_while_service_is_active(self):
        attempts = self.tmp / "attempts"
        self.command("trainer-runner", f'''n=0
[ ! -f "{attempts}" ] || n=$(cat "{attempts}")
n=$((n + 1))
printf '%s' "$n" > "{attempts}"
[ "$n" -ge 2 ]
''')
        runner = self.bin / "trainer-runner"
        self.command("busctl", '''printf '%s\n' \
  'com.steampowered.App20 123 helper user :1.2 user@1000.service - -' \
  'com.steampowered.App20.Instance123 123 helper user :1.2 user@1000.service - -'
''')
        env = self.env | {
            "FLING_WATCH_RUNNER": str(runner),
            "FLING_WATCH_MAX_ATTEMPTS": "2",
            "FLING_WATCH_RETRY_DELAY": "0",
            "FLING_WATCH_MIN_RUNTIME": "0",
        }
        retried = subprocess.run(
            ["/bin/bash", FLING, "_watch-run", "20"], env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(0, retried.returncode, retried.stdout + retried.stderr)
        self.assertEqual("2", attempts.read_text())
        self.assertIn("retrying", retried.stdout)

    def test_watcher_retries_a_successful_but_early_trainer_exit(self):
        attempts = self.tmp / "early-attempts"
        self.command("early-runner", f'''n=0
[ ! -f "{attempts}" ] || n=$(cat "{attempts}")
n=$((n + 1))
printf '%s' "$n" > "{attempts}"
exit 0
''')
        self.command("busctl", '''printf '%s\n' \
  'com.steampowered.App20 123 helper user :1.2 user@1000.service - -'
''')
        env = self.env | {
            "FLING_WATCH_RUNNER": str(self.bin / "early-runner"),
            "FLING_WATCH_MAX_ATTEMPTS": "2",
            "FLING_WATCH_RETRY_DELAY": "0",
            "FLING_WATCH_MIN_RUNTIME": "15",
        }
        retried = subprocess.run(
            ["/bin/bash", FLING, "_watch-run", "20"], env=env,
            text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.assertEqual(75, retried.returncode, retried.stdout + retried.stderr)
        self.assertEqual("2", attempts.read_text())
        self.assertIn("exited too soon", retried.stdout)

    def test_refresh_is_local_and_reports_current_state(self):
        data = self.payload(self.invoke("refresh", "20", "--json", check=True))
        self.assertTrue(data["success"]); self.assertEqual("refresh", data["operation"])
        self.assertEqual(20, data["appid"]); self.assertEqual(20, data["game"]["appid"]); self.assertFalse(data["game"]["trainer_installed"])

    def test_remove_deletes_only_one_safe_directory(self):
        one = self.home / "Trainers/10 - Quote Quest"; two = self.home / "Trainers/20 - Space Game"
        one.mkdir(parents=True); two.mkdir(parents=True)
        (one / "Trainer.exe").write_bytes(b"MZ"); (two / "Trainer.exe").write_bytes(b"MZ")
        data = self.payload(self.invoke("remove", "10", "--json", check=True))
        self.assertTrue(data["success"]); self.assertFalse(one.exists()); self.assertTrue(two.exists())

    def test_pragmata_remove_deletes_only_verified_managed_runtime(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        trainer = self.home / "Trainers/3357650 - PRAGMATA/Trainer.exe"
        trainer.parent.mkdir(parents=True); trainer.write_bytes(b"MZ trainer")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        dll = b"MZ managed runtime"
        (game / "dinput8.dll").write_bytes(dll)
        (game / ".fling-reframework.json").write_text(json.dumps({
            "appid": 3357650, "component": "REFramework", "installed_file": "dinput8.dll",
            "sha256": hashlib.sha256(dll).hexdigest(),
        }))

        data = self.payload(self.invoke("remove", "3357650", "--json", check=True))

        self.assertTrue(data["success"])
        self.assertFalse(trainer.parent.exists())
        self.assertFalse((game / "dinput8.dll").exists())
        self.assertFalse((game / ".fling-reframework.json").exists())

    def test_pragmata_remove_preserves_externally_changed_runtime_and_trainer(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        trainer = self.home / "Trainers/3357650 - PRAGMATA/Trainer.exe"
        trainer.parent.mkdir(parents=True); trainer.write_bytes(b"MZ trainer")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        original = b"MZ original runtime"
        changed = b"MZ externally changed runtime"
        (game / "dinput8.dll").write_bytes(changed)
        (game / ".fling-reframework.json").write_text(json.dumps({
            "appid": 3357650, "component": "REFramework", "installed_file": "dinput8.dll",
            "sha256": hashlib.sha256(original).hexdigest(),
        }))

        result = self.invoke("remove", "3357650", "--json")

        self.assertEqual(12, result.returncode)
        self.assertEqual("runtime_support_conflict", self.payload(result)["error_code"])
        self.assertTrue(trainer.exists())
        self.assertEqual(changed, (game / "dinput8.dll").read_bytes())

    def test_pragmata_remove_finishes_interrupted_runtime_cleanup(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        trainer = self.home / "Trainers/3357650 - PRAGMATA/Trainer.exe"
        trainer.parent.mkdir(parents=True); trainer.write_bytes(b"MZ trainer")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        dll_tomb = game / ".fling-remove-dinput8.dll"
        metadata_tomb = game / ".fling-remove-reframework.json"
        dll_tomb.write_bytes(b"MZ quarantined runtime")
        metadata_tomb.write_text("{}")

        data = self.payload(self.invoke("remove", "3357650", "--json", check=True))

        self.assertTrue(data["success"])
        self.assertFalse(trainer.parent.exists())
        self.assertFalse(dll_tomb.exists())
        self.assertFalse(metadata_tomb.exists())

    def test_remove_missing_and_invalid_args_have_stable_exits(self):
        p = self.invoke("remove", "abc", "--json"); self.assertEqual(2, p.returncode); self.assertFalse(self.payload(p)["success"]); self.assertEqual(0, self.payload(p)["appid"])
        p = self.invoke("remove", "999", "--json"); self.assertEqual(3, p.returncode); self.assertEqual(999, self.payload(p)["appid"])
        p = self.invoke("remove", "10", "--json"); self.assertEqual(7, p.returncode); self.assertEqual(10, self.payload(p)["appid"])

    def test_remove_refuses_symlink_directory(self):
        outside = self.tmp / "outside"; outside.mkdir()
        trainers = self.home / "Trainers"; trainers.mkdir()
        (trainers / "10 - Quote Quest").symlink_to(outside, target_is_directory=True)
        p = self.invoke("remove", "10", "--json")
        self.assertEqual(9, p.returncode); self.assertTrue(outside.exists())

    def test_remove_refuses_symlink_trainer_root_before_removal(self):
        outside = self.tmp / "external-trainers"
        victim = outside / "10 - Quote Quest"
        victim.mkdir(parents=True)
        (victim / "Trainer.exe").write_bytes(b"MZ")
        (self.home / "Trainers").symlink_to(outside, target_is_directory=True)
        p = self.invoke("remove", "10", "--json")
        self.assertEqual(9, p.returncode)
        self.assertEqual("unsafe_path", self.payload(p)["error_code"])
        self.assertTrue(victim.is_dir())

    def test_install_pe_writes_metadata_and_uses_hardened_curl(self):
        log = self.mock_download()
        data = self.payload(self.invoke("install", "20", "--json", check=True))
        self.assertTrue(data["success"]); self.assertEqual("Trainer installed successfully", data["message"])
        trainer = pathlib.Path(data["trainer_path"])
        self.assertTrue(trainer.is_file())
        metadata = json.loads((trainer.parent / "trainer-metadata.json").read_text())
        self.assertEqual(20, data["appid"]); self.assertEqual(20, metadata["appid"]); self.assertEqual(64, len(metadata["sha256"]))
        calls = log.read_text()
        for flag in ("--fail", "--location", "--connect-timeout", "--max-time"):
            self.assertIn(flag, calls)

    def test_pragmata_installs_only_reframework_dinput8_with_metadata(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        archive = self.tmp / "reframework.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", b"MZ reframework")
            zf.writestr("reframework_revision.txt", b"nightly-test")
        self.allow_test_reframework_archive(archive)
        curl_log = self.tmp / "reframework-curl.log"
        self.command("curl", f'''printf '%s\\n' "$*" >> "{curl_log}"
out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
cp "{archive}" "$out"
''')

        installed = self.invoke("_install-reframework", "3357650")

        self.assertEqual(0, installed.returncode, installed.stdout + installed.stderr)
        self.assertEqual(b"MZ reframework", (game / "dinput8.dll").read_bytes())
        self.assertFalse((game / "reframework_revision.txt").exists())
        metadata = json.loads((game / ".fling-reframework.json").read_text())
        self.assertEqual(3357650, metadata["appid"])
        self.assertEqual("dinput8.dll", metadata["installed_file"])
        self.assertEqual(64, len(metadata["sha256"]))
        calls = curl_log.read_text()
        self.assertIn("praydog/REFramework-nightly/releases/download/nightly-01391", calls)
        self.assertIn("--proto =https", calls)
        self.assertIn("--max-filesize 67108864", calls)

    def test_reframework_refuses_to_overwrite_unmanaged_dinput8(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        original = game / "dinput8.dll"
        original.write_bytes(b"unmanaged mod loader")
        archive = self.tmp / "reframework.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", b"MZ reframework")
        self.allow_test_reframework_archive(archive)
        self.command("curl", f'''out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
cp "{archive}" "$out"
''')

        refused = self.invoke("_install-reframework", "3357650")

        self.assertNotEqual(0, refused.returncode)
        self.assertIn("unmanaged dinput8.dll", (refused.stdout + refused.stderr).lower())
        self.assertEqual(b"unmanaged mod loader", original.read_bytes())
        self.assertFalse((game / ".fling-reframework.json").exists())

    def test_reframework_refuses_identical_unmanaged_dinput8(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        dll = b"MZ same official bytes"
        original = game / "dinput8.dll"
        original.write_bytes(dll)
        archive = self.tmp / "same-reframework.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", dll)
        self.allow_test_reframework_archive(archive)
        self.command("curl", f'''out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
cp "{archive}" "$out"
''')

        refused = self.invoke("_install-reframework", "3357650")

        self.assertNotEqual(0, refused.returncode)
        self.assertIn("unmanaged dinput8.dll", (refused.stdout + refused.stderr).lower())
        self.assertEqual(dll, original.read_bytes())
        self.assertFalse((game / ".fling-reframework.json").exists())

    def test_reframework_rejects_archive_checksum_mismatch(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        archive = self.tmp / "tampered-reframework.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", b"MZ tampered")
        self.env["FLING_TESTING"] = "1"
        self.env["FLING_REFRAMEWORK_SHA256"] = "0" * 64
        self.command("curl", f'''out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
cp "{archive}" "$out"
''')

        refused = self.invoke("_install-reframework", "3357650")

        self.assertNotEqual(0, refused.returncode)
        self.assertIn("checksum mismatch", (refused.stdout + refused.stderr).lower())
        self.assertFalse((game / "dinput8.dll").exists())

    def test_reframework_recovers_pending_metadata_after_interruption(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        dll = b"MZ recovered reframework"
        digest = hashlib.sha256(dll).hexdigest()
        (game / "dinput8.dll").write_bytes(dll)
        (game / ".fling-reframework.json").write_text(json.dumps({
            "appid": 3357650, "installed_file": "dinput8.dll", "sha256": "0" * 64,
        }))
        (game / ".fling-reframework.pending.json").write_text(json.dumps({
            "schema_version": 1, "appid": 3357650, "component": "REFramework",
            "installed_file": "dinput8.dll", "sha256": digest,
        }))
        archive = self.tmp / "recover-reframework.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", dll)
        self.allow_test_reframework_archive(archive)
        self.command("curl", f'''out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
cp "{archive}" "$out"
''')

        recovered = self.invoke("_install-reframework", "3357650")

        self.assertEqual(0, recovered.returncode, recovered.stdout + recovered.stderr)
        self.assertFalse((game / ".fling-reframework.pending.json").exists())
        metadata = json.loads((game / ".fling-reframework.json").read_text())
        self.assertEqual(digest, metadata["sha256"])

    def test_reframework_automation_is_scoped_to_pragmata(self):
        game = self.lib2 / "steamapps/common/Space Game"
        game.mkdir(parents=True)

        refused = self.invoke("_install-reframework", "20")

        self.assertNotEqual(0, refused.returncode)
        self.assertIn("not enabled", (refused.stdout + refused.stderr).lower())
        self.assertFalse((game / "dinput8.dll").exists())

    def test_reframework_rejects_manifest_install_dir_escape(self):
        outside = self.lib2 / "steamapps/outside-game"
        outside.mkdir()
        self.manifest(self.lib2, "3357650", "PRAGMATA", "../outside-game")

        refused = self.invoke("_install-reframework", "3357650")

        self.assertNotEqual(0, refused.returncode)
        self.assertIn("not found safely", (refused.stdout + refused.stderr).lower())
        self.assertFalse((outside / "dinput8.dll").exists())

    def test_reframework_rejects_symlinked_manifest_game_directory(self):
        common = self.lib2 / "steamapps/common"
        real_game = common / "RealPRAGMATA"
        real_game.mkdir(parents=True)
        (common / "PRAGMATA").symlink_to(real_game, target_is_directory=True)
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")

        refused = self.invoke("_install-reframework", "3357650")

        self.assertNotEqual(0, refused.returncode)
        self.assertIn("not found safely", (refused.stdout + refused.stderr).lower())
        self.assertFalse((real_game / "dinput8.dll").exists())

    def test_pragmata_public_installs_apply_runtime_support(self):
        source = FLING.read_text()
        get_body = source[source.index("cmd_get() {"):source.index("json_failure() {")]
        json_body = source[source.index("cmd_install_json() {"):source.index("cmd_auto() {")]
        self.assertIn('cmd_install_json "$appid"', get_body)
        self.assertIn('install_runtime_support "$appid"', json_body)

    def test_pragmata_json_install_is_single_json_and_installs_reframework(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        archive = self.tmp / "reframework-integration.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", b"MZ integrated reframework")
        self.allow_test_reframework_archive(archive)
        self.command("curl", f'''out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
case "$*" in
  *%\\{{http_code\\}}*) printf '200' ;;
  *downloads/mock.bin*) printf 'MZ trainer' > "$out" ;;
  *REFramework.zip*) cp "{archive}" "$out" ;;
  *) printf '<a href="https://flingtrainer.com/downloads/mock.bin">trainer</a>' ;;
esac
''')
        self.command("file", "printf '%s\\n' 'PE32 executable'\n")

        result = self.invoke("install", "3357650", "--json")
        data = self.payload(result)

        self.assertEqual(0, result.returncode, result.stderr)
        self.assertTrue(data["success"])
        self.assertEqual(3357650, data["appid"])
        self.assertEqual(b"MZ integrated reframework", (game / "dinput8.dll").read_bytes())
        self.assertEqual(1, len(result.stdout.splitlines()))

    def test_pragmata_runtime_failure_preserves_existing_trainer(self):
        self.manifest(self.lib2, "3357650", "PRAGMATA", "PRAGMATA")
        game = self.lib2 / "steamapps/common/PRAGMATA"
        game.mkdir(parents=True)
        old_trainer = self.home / "Trainers/3357650 - PRAGMATA/Trainer.exe"
        old_trainer.parent.mkdir(parents=True)
        old_trainer.write_bytes(b"MZ previous trainer")
        archive = self.tmp / "bad-reframework.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("dinput8.dll", b"MZ unsupported archive")
        self.env["FLING_TESTING"] = "1"
        self.env["FLING_REFRAMEWORK_SHA256"] = "0" * 64
        self.command("curl", f'''out=""; prev=""
for arg in "$@"; do [ "$prev" = -o ] && out="$arg"; prev="$arg"; done
case "$*" in
  *%\\{{http_code\\}}*) printf '200' ;;
  *downloads/mock.bin*) printf 'MZ replacement trainer' > "$out" ;;
  *REFramework.zip*) cp "{archive}" "$out" ;;
  *) printf '<a href="https://flingtrainer.com/downloads/mock.bin">trainer</a>' ;;
esac
''')
        self.command("file", "printf '%s\\n' 'PE32 executable'\n")

        result = self.invoke("install", "3357650", "--json")
        data = self.payload(result)

        self.assertEqual(11, result.returncode)
        self.assertEqual("runtime_support_failed", data["error_code"])
        self.assertNotIn(">>>", data["message"])
        self.assertEqual(b"MZ previous trainer", old_trainer.read_bytes())
        self.assertEqual([], list(old_trainer.parent.parent.glob(".fling-install-*")))

    def test_install_confines_malicious_manifest_name_and_preserves_display_name(self):
        name = '雪 " Quest/../../../escaped\ncontrol'
        self.manifest(self.steam, "30", name, "Malicious")
        self.mock_download()
        data = self.payload(self.invoke("install", "30", "--json", check=True))
        trainer = pathlib.Path(data["trainer_path"])
        root = self.home / "Trainers"
        self.assertEqual(name, data["name"])
        self.assertEqual(root.resolve(), trainer.parent.parent.resolve())
        self.assertNotIn("/", trainer.parent.name)
        self.assertNotIn("\n", trainer.parent.name)
        self.assertFalse((self.home / "escaped").exists())
        self.assertFalse((self.tmp / "escaped").exists())
        metadata = json.loads((trainer.parent / "trainer-metadata.json").read_text())
        self.assertEqual(name, metadata["game_name"])

    def test_install_refuses_symlink_destination_under_regular_root(self):
        outside = self.tmp / "external-destination"
        outside.mkdir()
        root = self.home / "Trainers"
        root.mkdir()
        (root / "20 - Space Game").symlink_to(outside, target_is_directory=True)
        self.mock_download()
        p = self.invoke("install", "20", "--json")
        self.assertEqual(9, p.returncode)
        self.assertEqual("unsafe_path", self.payload(p)["error_code"])
        self.assertEqual([], list(outside.iterdir()))

    def test_install_refuses_symlink_trainer_root(self):
        outside = self.tmp / "external-trainers"
        outside.mkdir()
        (self.home / "Trainers").symlink_to(outside, target_is_directory=True)
        self.mock_download()
        p = self.invoke("install", "20", "--json")
        self.assertEqual(9, p.returncode)
        self.assertEqual("unsafe_path", self.payload(p)["error_code"])
        self.assertEqual([], list(outside.iterdir()))

    def test_legacy_get_and_auto_confine_malicious_manifest_name(self):
        self.enable_legacy_pcre_grep()
        self.manifest(self.steam, "30", "../../escaped", "Malicious")
        self.mock_download()
        for command in ("get", "auto"):
            with self.subTest(command=command):
                p = self.invoke(command, "30")
                self.assertEqual(0, p.returncode, p.stderr)
                self.assertFalse((self.home / "escaped").exists())
                trainer = next((self.home / "Trainers").glob("30 - */Trainer.exe"))
                self.assertEqual((self.home / "Trainers").resolve(),
                                 trainer.parent.parent.resolve())

    def test_legacy_get_rejects_unsafe_zip_members_before_extraction(self):
        self.enable_legacy_pcre_grep()
        cases = {
            "traversal": "../../escaped.exe",
            "absolute": str(self.tmp / "absolute-escaped.exe"),
            "backslash": "..\\backslash-escaped.exe",
            "symlink": "Trainer.exe",
        }
        for kind, member in cases.items():
            with self.subTest(kind=kind):
                archive = self.tmp / f"legacy-{kind}.zip"
                with zipfile.ZipFile(archive, "w") as zf:
                    if kind == "symlink":
                        info = zipfile.ZipInfo(member)
                        info.create_system = 3
                        info.external_attr = (stat.S_IFLNK | 0o777) << 16
                        zf.writestr(info, str(self.tmp / "symlink-target"))
                    else:
                        zf.writestr(member, b"MZ escaped")
                self.mock_download("Zip archive data", payload_path=archive)
                p = self.invoke("get", "20")
                self.assertNotEqual(0, p.returncode)
                self.assertIn("unsafe", (p.stdout + p.stderr).lower())
                self.assertFalse((self.tmp / "escaped.exe").exists())
                self.assertFalse((self.tmp / "absolute-escaped.exe").exists())
                self.assertFalse((self.home / "Trainers/backslash-escaped.exe").exists())
                shutil.rmtree(self.home / "Trainers", ignore_errors=True)

    def test_zip_resource_limits_reject_before_extraction(self):
        cases = {}
        archive = self.tmp / "too-many.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            for i in range(1025): zf.writestr(f"{i}.txt", b"x")
        cases["member count"] = archive
        archive = self.tmp / "large-member.zip"
        with zipfile.ZipFile(archive, "w") as zf: zf.writestr("Trainer.exe", b"x")
        self.patch_zip_sizes(archive, 256 * 1024 * 1024 + 1, 2 * 1024 * 1024)
        cases["member size"] = archive
        archive = self.tmp / "large-total.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("one.exe", b"x"); zf.writestr("two.exe", b"x")
        self.patch_zip_sizes(archive, 256 * 1024 * 1024, 2 * 1024 * 1024)
        cases["aggregate size"] = archive
        archive = self.tmp / "high-ratio.zip"
        with zipfile.ZipFile(archive, "w") as zf: zf.writestr("Trainer.exe", b"x")
        self.patch_zip_sizes(archive, 10 * 1024 * 1024, 1)
        cases["compression ratio"] = archive
        for label, payload in cases.items():
            with self.subTest(limit=label):
                self.mock_download("Zip archive data", payload_path=payload)
                p = self.invoke("install", "20", "--json")
                self.assertEqual(6, p.returncode)
                self.assertEqual("invalid_file", self.payload(p)["error_code"])
                shutil.rmtree(self.home / "Trainers", ignore_errors=True)

    def test_legacy_get_empty_zip_fails_clearly_and_cleans_destination(self):
        self.enable_legacy_pcre_grep()
        archive = self.tmp / "no-exe.zip"
        with zipfile.ZipFile(archive, "w") as zf: zf.writestr("readme.txt", b"hello")
        self.mock_download("Zip archive data", payload_path=archive)
        p = self.invoke("get", "20")
        self.assertNotEqual(0, p.returncode)
        self.assertIn("no executable", (p.stdout + p.stderr).lower())
        self.assertFalse(any((self.home / "Trainers").glob("20 - *")))

    def test_public_commands_find_normalized_trainer_directory(self):
        self.enable_legacy_pcre_grep()
        name = "Slash/Game"
        self.manifest(self.steam, "30", name, "Slash Game")
        trainer = self.home / "Trainers/30 - Slash_Game/Trainer.exe"
        trainer.parent.mkdir(parents=True); trainer.write_bytes(b"MZ")
        (self.steam / "steamapps/compatdata/30/pfx").mkdir(parents=True)
        launcher = self.steam / "steamapps/common/SteamLinuxRuntime_test/pressure-vessel/bin/steam-runtime-launch-client"
        launcher.parent.mkdir(parents=True); launcher.write_text("")
        self.command("protontricks-launch", "exit 0\n")
        self.command("pgrep", "exit 1\n")
        self.command("busctl", "exit 1\n")
        run = self.invoke("run", "30")
        self.assertEqual(0, run.returncode, run.stderr)
        setup = self.invoke("setup", "30")
        self.assertNotIn("no trainer downloaded", setup.stdout)

    def test_install_zip_normalizes_exe(self):
        archive = self.tmp / "trainer.zip"
        with zipfile.ZipFile(archive, "w") as zf:
            zf.writestr("a.exe", b"small")
            zf.writestr("nested/b.exe", b"largest-payload")
        self.mock_download("Zip archive data", payload_path=archive)
        data = self.payload(self.invoke("install", "20", "--json", check=True))
        self.assertTrue(pathlib.Path(data["trainer_path"]).is_file())
        self.assertEqual(b"largest-payload", pathlib.Path(data["trainer_path"]).read_bytes())

    def test_install_rejects_unsafe_zip_members_before_extraction(self):
        for kind in ("traversal", "symlink"):
            with self.subTest(kind=kind):
                archive = self.tmp / f"{kind}.zip"
                outside = self.tmp / f"{kind}-escaped.exe"
                with zipfile.ZipFile(archive, "w") as zf:
                    if kind == "traversal":
                        zf.writestr(f"../../{outside.name}", b"MZ escaped")
                    else:
                        info = zipfile.ZipInfo("Trainer.exe")
                        info.create_system = 3
                        info.external_attr = (stat.S_IFLNK | 0o777) << 16
                        zf.writestr(info, str(outside))
                self.mock_download("Zip archive data", payload_path=archive)
                p = self.invoke("install", "20", "--json")
                self.assertEqual(6, p.returncode)
                self.assertEqual("invalid_file", self.payload(p)["error_code"])
                self.assertFalse(outside.exists())

    def test_install_failures_are_json_with_stable_exits(self):
        self.mock_download("HTML document")
        p = self.invoke("install", "20", "--json"); self.assertEqual(6, p.returncode); self.assertEqual("invalid_file", self.payload(p)["error_code"])
        self.mock_download(page_code="404")
        p = self.invoke("install", "20", "--json"); self.assertEqual(4, p.returncode)
        self.mock_download(link=False)
        p = self.invoke("install", "20", "--json"); self.assertEqual(4, p.returncode)


if __name__ == "__main__": unittest.main(verbosity=2)
