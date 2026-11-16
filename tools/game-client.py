#!/usr/bin/env python3
"""Locate and provision the compiled research client; Python standard library only."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".local/game-client.json"
DEFAULT_CLIENT = ROOT / ".local/game-client"
SETUP = "Run python3 tools/game-client.py use /path/to/World_of_Tanks or install."


def selected_client():
    override = os.environ.get("WOT_GAME_DIR")
    if override:
        return Path(override).expanduser().resolve()
    if CONFIG.exists():
        data = json.loads(CONFIG.read_text(encoding="utf-8"))
        value = data.get("path") if isinstance(data, dict) else None
        if not isinstance(value, str) or not value or not Path(value).is_absolute():
            raise ValueError(f"{CONFIG} must contain an absolute 'path' string")
        return Path(value).resolve()
    return DEFAULT_CLIENT.resolve()


def validate_client(path):
    missing = [name for name in ("WorldOfTanks.exe", "res", "res/packages")
               if not (path / name).exists()]
    if missing:
        raise ValueError(f"Client unavailable at {path}: missing {', '.join(missing)}. {SETUP}")
    if not (path / "WorldOfTanks.exe").is_file() or not (path / "res/packages").is_dir():
        raise ValueError(f"Invalid client layout at {path}. {SETUP}")


def remember_client(path):
    CONFIG.parent.mkdir(parents=True, exist_ok=True)
    pending = CONFIG.with_suffix(".tmp")
    pending.write_text(json.dumps({"path": str(path)}, indent=2) + "\n", encoding="utf-8")
    pending.replace(CONFIG)


def launcher(*args):
    executable = shutil.which("wg-launcher")
    if executable is None:
        raise ValueError("wg-launcher is missing from PATH; see docs/game-client.md for setup")
    subprocess.run([executable, *map(str, args)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("path", help="Print the validated absolute client path (offline)")
    status = commands.add_parser("status", help="Check local files and hash the executable (offline)")
    status.add_argument("--json", action="store_true", help="Emit a machine-readable report")
    use = commands.add_parser("use", help="Register an existing client without modifying it")
    use.add_argument("path", type=Path)
    install = commands.add_parser("install", help="Download a client using wg-launcher")
    install.add_argument("--output", type=Path, help="Destination; defaults to the selected path")
    install.add_argument("--region", default="eu")
    install.add_argument("--language", default="en")
    install.add_argument("--client-type")
    commands.add_parser("check-updates", help="Ask wg-launcher for installed/latest versions (network)")
    commands.add_parser("update", help="Update the selected client using wg-launcher (network)")
    args = parser.parse_args()

    try:
        if args.command == "use":
            path = args.path.expanduser().resolve()
            validate_client(path)
            remember_client(path)
            print(f"Registered {path}")
            if os.environ.get("WOT_GAME_DIR"):
                print("WOT_GAME_DIR still overrides this saved selection.", file=sys.stderr)
            return 0

        path = (args.output.expanduser().resolve()
                if args.command == "install" and args.output else selected_client())
        if args.command == "install":
            if (path / "WorldOfTanks.exe").exists():
                raise ValueError(f"Client already exists at {path}; use 'use' or 'update' instead")
            options = ["install", "--game", "wot", "--region", args.region,
                       "--language", args.language, "--output", path]
            if args.client_type:
                options.extend(["--client-type", args.client_type])
            launcher(*options)
            validate_client(path)
            remember_client(path)
            print(f"Registered {path}")
            return 0

        validate_client(path)
        if args.command == "path":
            print(path)
        elif args.command == "status":
            with (path / "WorldOfTanks.exe").open("rb") as executable:
                digest = hashlib.file_digest(executable, "sha256").hexdigest()
            report = {
                "path": str(path),
                "executable": str(path / "WorldOfTanks.exe"),
                "resources": str(path / "res"),
                "executable_sha256": digest,
                "version_file": str(path / "version.xml") if (path / "version.xml").is_file() else None,
            }
            if args.json:
                print(json.dumps(report, indent=2))
            else:
                for key, value in report.items():
                    print(f"{key}: {value}")
                print("Local layout checked; package completeness and server compatibility are unverified.")
                print("Run 'check-updates' to query available versions.")
        else:
            launcher("status" if args.command == "check-updates" else "update", "--path", path)
        return 0
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"game-client: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
