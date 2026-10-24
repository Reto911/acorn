#!/usr/bin/env python3
"""Build and install Acorn, or independently initialize an existing empty database."""

import argparse
import getpass
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parent.parent
SERVICES = ("backend", "worker")
SQL_SCRIPTS = (
    Path("sql/001-initial-schema.sql"),
    Path("examples/seed-user.sql"),
    Path("sql/002-example-problem.sql"),
)


def arguments():
    """Select one mode and reject deployment options in database-only mode."""
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--dest", type=Path, help="deployment directory outside the checkout")
    mode.add_argument("--init-db", action="store_true", help="only initialize an empty database")
    parser.add_argument(
        "--profile", choices=("release", "debug"),
        help="build profile (default: release)",
    )
    parser.add_argument(
        "--kafka-link", choices=("dynamic", "static"),
        help="librdkafka linkage (default: dynamic)",
    )
    parser.add_argument("--backend-config", type=Path, help="replace backend.toml with this file")
    parser.add_argument("--worker-config", type=Path, help="replace worker.toml with this file")
    args = parser.parse_args()
    if args.init_db:
        if any((args.profile, args.kafka_link, args.backend_config, args.worker_config)):
            parser.error("--init-db cannot be combined with deployment options")
    else:
        args.dest = args.dest.expanduser().resolve()
        if args.dest.is_relative_to(ROOT):
            parser.error("--dest must be outside the checkout to keep build inputs separate")
        for service in SERVICES:
            source = getattr(args, f"{service}_config")
            if source is not None:
                source = source.expanduser().resolve()
                if not source.is_file():
                    parser.error(f"configuration file does not exist: {source}")
                setattr(args, f"{service}_config", source)
    return args


def build(profile, kafka_link):
    """Build both services and the frontend before changing deployment files."""
    command = [
        "cargo", "build", "--workspace", "--locked",
        "--message-format=json-render-diagnostics",
    ]
    if profile == "release":
        command.append("--release")
    if kafka_link == "static":
        command.extend([
            "--no-default-features",
            "--features", "acorn-backend/kafka-static,acorn-worker/kafka-static",
        ])
    print(f"Building Rust services ({profile}, Kafka {kafka_link})...", flush=True)
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True, check=True)
    binaries = {}
    for line in result.stdout.splitlines():
        artifact = json.loads(line)
        if artifact["reason"] == "compiler-artifact" and artifact.get("executable"):
            binaries[artifact["target"]["name"]] = Path(artifact["executable"])
    for service in SERVICES:
        if f"acorn-{service}" not in binaries:
            raise RuntimeError(f"Cargo did not report the acorn-{service} executable")
    print("Building frontend...", flush=True)
    subprocess.run(["pnpm", "install", "--frozen-lockfile"], cwd=ROOT / "web", check=True)
    subprocess.run(["pnpm", "build"], cwd=ROOT / "web", check=True)
    return binaries


def copy_file(source, destination, mode):
    """Replace one installed file, keeping private configuration at mode 0600."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".acorn-copy-", dir=destination.parent) as directory:
        staged = Path(directory) / destination.name
        shutil.copyfile(source, staged)
        staged.chmod(mode)
        staged.replace(destination)


def install(destination, binaries, configs):
    """Copy owned artifacts while preserving existing configuration unless explicitly supplied."""
    for service in SERVICES:
        name = f"acorn-{service}"
        copy_file(binaries[name], destination / "bin" / name, 0o755)
        example = ROOT / "config" / f"{service}.example.toml"
        copy_file(example, destination / "config" / example.name, 0o644)
        active = destination / "config" / f"{service}.toml"
        if configs[service] is not None:
            copy_file(configs[service], active, 0o600)
        elif not (active.exists() or active.is_symlink()):
            copy_file(example, active, 0o600)
        else:
            print(f"Preserved configuration: {active}")
    for script in SQL_SCRIPTS:
        copy_file(ROOT / script, destination / script, 0o644)
    static = destination / "web" / "dist"
    if static.exists():
        shutil.rmtree(static)
    shutil.copytree(ROOT / "web" / "dist", static)
    print(f"Installed Acorn in {destination}")
    print("Review config/backend.toml and config/worker.toml before starting services.")
    print("Supply MYSQL_PASSWORD in the backend process environment.")
    for service in SERVICES:
        print(shlex.join([
            str(destination / "bin" / f"acorn-{service}"),
            "--config", str(destination / "config" / f"{service}.toml"),
        ]))
    print(f"Serve {static} with a static server and proxy /api/ to the backend.")


def mysql_option(value):
    """Quote a MySQL option-file value, including quotes and control characters."""
    escaped = value.translate(str.maketrans({
        "\\": "\\\\", '"': '\\"', "\n": "\\n", "\r": "\\r", "\t": "\\t", "\b": "\\b",
    }))
    return f'"{escaped}"'


def initialize_database():
    """Prompt independently of deployment and initialize an empty database, failing on SQL errors."""
    if shutil.which("mysql") is None:
        raise RuntimeError("MySQL 8.4 client 'mysql' must be available in PATH")
    host = input("MySQL host [127.0.0.1]: ").strip() or "127.0.0.1"
    port = int(input("MySQL port [3306]: ").strip() or "3306")
    if not 1 <= port <= 65535:
        raise ValueError("MySQL port must be between 1 and 65535")
    database = input("Existing empty database: ").strip()
    username = input("Initialization account: ").strip()
    if not database or not username:
        raise ValueError("Database and initialization account are required")
    password = getpass.getpass("Initialization password: ")
    with tempfile.TemporaryDirectory(prefix="acorn-mysql-") as directory:
        credentials = Path(directory) / "client.cnf"
        with credentials.open("x", encoding="utf-8") as output:
            credentials.chmod(0o600)
            output.write(f"[client]\npassword={mysql_option(password)}\n")
        command = [
            "mysql", f"--defaults-file={credentials}", "--no-login-paths",
            "--protocol=TCP", f"--host={host}", f"--port={port}", f"--user={username}",
            f"--database={database}", "--ssl-mode=DISABLED", "--default-character-set=utf8mb4",
            "--batch", "--skip-column-names",
        ]
        query = (
            "SELECT COUNT(*) FROM information_schema.tables "
            "WHERE table_schema = DATABASE()"
        )
        result = subprocess.run(
            command + [f"--execute={query}"],
            stdout=subprocess.PIPE, text=True, check=True,
        )
        if int(result.stdout.strip()) != 0:
            raise RuntimeError(f"Database {database!r} is not empty; initialization refused")
        for script in SQL_SCRIPTS:
            print(f"Initializing {script}...", flush=True)
            with (ROOT / script).open("rb") as source:
                subprocess.run(command, stdin=source, check=True)
    print(f"Initialized {database!r}: schema version 4, user 1 and example problem 1 (104 cases).")
    print("Configure the backend with this database and its separate runtime account.")


def main():
    """Run exactly one selected mode and retain failing command exit status."""
    args = arguments()
    try:
        if args.init_db:
            initialize_database()
        else:
            binaries = build(args.profile or "release", args.kafka_link or "dynamic")
            configs = {
                service: getattr(args, f"{service}_config") for service in SERVICES
            }
            install(args.dest, binaries, configs)
    except subprocess.CalledProcessError as error:
        print(f"Failed: {error.cmd[0]} exited with status {error.returncode}.", file=sys.stderr)
        return error.returncode if error.returncode > 0 else 1
    except (OSError, ValueError, RuntimeError, EOFError) as error:
        print(f"Failed: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("Cancelled.", file=sys.stderr)
        return 130
    return 0


if __name__ == "__main__":
    sys.exit(main())
