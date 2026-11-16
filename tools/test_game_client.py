import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("game_client", Path(__file__).with_name("game-client.py"))
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)


class GameClientTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.config = self.root / ".local/game-client.json"
        self.default = self.root / ".local/game-client"
        self.stack = contextlib.ExitStack()
        self.addCleanup(self.stack.close)
        self.stack.enter_context(patch.object(client, "CONFIG", self.config))
        self.stack.enter_context(patch.object(client, "DEFAULT_CLIENT", self.default))
        self.stack.enter_context(patch.dict(os.environ))
        os.environ.pop("WOT_GAME_DIR", None)

    def fixture(self, name="World of Tanks"):
        path = self.root / name
        (path / "res/packages").mkdir(parents=True)
        (path / "WorldOfTanks.exe").write_bytes(b"fixture executable")
        return path.resolve()

    def invoke(self, *args):
        stdout, stderr = io.StringIO(), io.StringIO()
        with patch("sys.argv", ["game-client.py", *map(str, args)]), \
                contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            code = client.main()
        return code, stdout.getvalue(), stderr.getvalue()

    def test_missing_client_fails_with_setup_instructions(self):
        code, stdout, stderr = self.invoke("path")
        self.assertEqual(code, 1)
        self.assertEqual(stdout, "")
        self.assertIn("use /path/to/World_of_Tanks or install", stderr)

    def test_registration_path_and_offline_identity(self):
        path = self.fixture()
        self.assertEqual(self.invoke("use", path)[0], 0)
        self.assertEqual(self.invoke("path")[:2], (0, f"{path}\n"))
        with patch.object(client, "launcher", side_effect=AssertionError("must stay offline")):
            code, output, _ = self.invoke("status", "--json")
        self.assertEqual(code, 0)
        report = json.loads(output)
        self.assertEqual(report["path"], str(path))
        self.assertEqual(report["executable_sha256"], client.hashlib.sha256(b"fixture executable").hexdigest())

    def test_environment_overrides_saved_selection_without_fallback(self):
        self.invoke("use", self.fixture())
        other = self.fixture("other")
        os.environ["WOT_GAME_DIR"] = str(other)
        self.assertEqual(self.invoke("path")[:2], (0, f"{other}\n"))
        os.environ["WOT_GAME_DIR"] = str(self.root / "missing")
        self.assertEqual(self.invoke("path")[0], 1)

    def test_invalid_registration_keeps_previous_selection(self):
        path = self.fixture()
        self.invoke("use", path)
        self.assertEqual(self.invoke("use", self.root / "missing")[0], 1)
        self.assertEqual(client.selected_client(), path)

    def test_malformed_config_reports_error(self):
        self.config.parent.mkdir()
        for content in ("{", "[]", '{"path": "relative"}'):
            self.config.write_text(content)
            self.assertEqual(self.invoke("path")[0], 1)

    def test_install_delegates_explicit_game_and_registers_after_success(self):
        path = self.root / "download"

        def install(*args):
            self.assertEqual(args, ("install", "--game", "wot", "--region", "eu",
                                    "--language", "en", "--output", path.resolve()))
            self.fixture("download")

        with patch.object(client, "launcher", side_effect=install):
            self.assertEqual(self.invoke("install", "--output", path)[0], 0)
        self.assertEqual(client.selected_client(), path.resolve())

    def test_failed_install_preserves_selection_and_existing_client_is_protected(self):
        path = self.fixture()
        self.invoke("use", path)
        with patch.object(client, "launcher", side_effect=subprocess.CalledProcessError(2, "wg-launcher")) as run:
            self.assertEqual(self.invoke("install")[0], 1)
            run.assert_not_called()
            self.assertEqual(self.invoke("install", "--output", self.root / "new")[0], 1)
        self.assertEqual(client.selected_client(), path)

    def test_update_and_check_use_selected_installation(self):
        path = self.fixture()
        self.invoke("use", path)
        with patch.object(client, "launcher") as run:
            self.assertEqual(self.invoke("check-updates")[0], 0)
            run.assert_called_with("status", "--path", path)
            self.assertEqual(self.invoke("update")[0], 0)
            run.assert_called_with("update", "--path", path)

    def test_missing_launcher_is_actionable_and_subprocess_uses_argument_array(self):
        with patch.object(client.shutil, "which", return_value=None):
            with self.assertRaisesRegex(ValueError, "missing from PATH"):
                client.launcher("install")
        with patch.object(client.shutil, "which", return_value="/bin/wg-launcher"), \
                patch.object(client.subprocess, "run") as run:
            client.launcher("update", "--path", self.root / "spaces and $shell")
            run.assert_called_once_with(
                ["/bin/wg-launcher", "update", "--path", str(self.root / "spaces and $shell")], check=True)


if __name__ == "__main__":
    unittest.main()
