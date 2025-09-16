#!/usr/bin/env python3
"""
List tables in TiDB using the Homebrew mysql client and DATABASE_URL from .env.

Usage:
  1) Put a .env file in the repository root with a line like:
       DATABASE_URL=mysql://user:password@127.0.0.1:4000/dbname?ssl-mode=DISABLED
     (Adjust host/port/db and SSL as needed. TiDB default port is 4000.)
  2) Run:
       python3 list.py

If the database is specified in DATABASE_URL, the script lists tables in that schema.
Otherwise it lists all non-system schemas.
"""

from __future__ import annotations

import os
import re
import shlex
import shutil
import subprocess
import sys
from typing import Dict, Optional, Tuple

try:
    # Prefer stdlib if available; no external deps required
    from urllib.parse import urlparse, parse_qs, unquote
except Exception:  # pragma: no cover
    print("FATAL: Python stdlib urllib.parse unavailable", file=sys.stderr)
    sys.exit(2)


def load_dotenv_into_environ(dotenv_path: str) -> None:
    """Load a simple .env into os.environ (no external deps).

    Supports lines in KEY=VALUE format, ignores comments and blank lines.
    Does not handle export keyword or complex quoting/escapes.
    """
    if not os.path.isfile(dotenv_path):
        return
    try:
        with open(dotenv_path, "r", encoding="utf-8") as f:
            for raw in f:
                line = raw.strip()
                if not line or line.startswith("#"):
                    continue
                if "=" not in line:
                    continue
                key, value = line.split("=", 1)
                key = key.strip()
                value = value.strip().strip('"').strip("'")
                if key and key not in os.environ:
                    os.environ[key] = value
    except Exception as e:  # pragma: no cover
        print(f"Warning: failed to read .env: {e}", file=sys.stderr)


def find_mysql_binary() -> Optional[str]:
    """Locate the mysql client binary.

    Prefer the Oracle MySQL client from Homebrew (mysql-client) to avoid auth plugin
    incompatibilities with MariaDB clients.
    """
    candidates = [
        	# Homebrew mysql-client preferred locations
        	"/opt/homebrew/opt/mysql-client/bin/mysql",
        	"/usr/local/opt/mysql-client/bin/mysql",
        	# PATH discovery
        	shutil.which("mysql"),
        	# Fallback common locations
        	"/opt/homebrew/bin/mysql",
        	"/usr/local/bin/mysql",
    ]
    for candidate in candidates:
        if not candidate:
            continue
        if (os.path.exists(candidate) or os.path.islink(candidate)) and os.access(candidate, os.X_OK):
            return candidate
        # shutil.which already verified executability if non-None
        if candidate == shutil.which("mysql") and candidate is not None:
            return candidate
    return None


def find_mysql_plugin_dir() -> Optional[str]:
    """Return the mysql-client plugin directory if available."""
    for path in (
        	"/opt/homebrew/opt/mysql-client/lib/plugin",
        	"/usr/local/opt/mysql-client/lib/plugin",
        	"/opt/homebrew/Cellar/mysql-client/8.*/lib/plugin",  # glob-like hint (not expanded here)
    ):
        if os.path.isdir(path):
            return path
    # Fallback to server plugin dir if present
    for path in ("/opt/homebrew/Cellar/mysql/9.4.0_3/lib/plugin", "/opt/homebrew/lib/plugin"):
        if os.path.isdir(path):
            return path
    return None


def normalize_host_port_from_netloc(netloc: str) -> Tuple[str, Optional[int]]:
    """Handle netloc variations such as tcp(host:port) used by some DSNs."""
    # Strip potential userinfo if present (should be handled by urlparse already, but just in case)
    # Patterns like tcp(127.0.0.1:4000)
    m = re.search(r"tcp\((?P<host>[^:]+)(?::(?P<port>\d+))?\)", netloc)
    if m:
        host = m.group("host")
        port = int(m.group("port")) if m.group("port") else None
        return host, port
    # Fallback: let urllib.parse handle host/port normally via urlparse
    return netloc, None


def parse_database_url(url: str) -> Dict[str, Optional[str]]:
    # Accept jdbc:mysql://... by stripping jdbc:
    if url.startswith("jdbc:"):
        url = url[len("jdbc:") :]

    parsed = urlparse(url)

    # Some DSNs might encode tcp(host:port) in netloc. Try to normalize.
    host_override: Optional[str] = None
    port_override: Optional[int] = None
    if parsed.netloc and "tcp(" in parsed.netloc:
        host_override, port_override = normalize_host_port_from_netloc(parsed.netloc)

    username = unquote(parsed.username or "")
    password = unquote(parsed.password or "")
    host = host_override or parsed.hostname or "127.0.0.1"
    port = port_override or (parsed.port or 4000)
    database = (parsed.path or "").lstrip("/") or None
    params = parse_qs(parsed.query)

    # Extract SSL parameters, normalizing common synonyms
    ssl_mode = (params.get("ssl-mode") or params.get("sslmode") or params.get("ssl") or [""])[0]
    ssl_ca = (params.get("ssl-ca") or params.get("sslrootcert") or [""])[0]
    ssl_mode_aliases = {
        "disable": "DISABLED",
        "disabled": "DISABLED",
        "preferred": "PREFERRED",
        "require": "REQUIRED",
        "required": "REQUIRED",
        "verify-ca": "VERIFY_CA",
        "verify_full": "VERIFY_IDENTITY",
        "verify-identity": "VERIFY_IDENTITY",
    }
    ssl_mode = ssl_mode_aliases.get(ssl_mode.lower(), ssl_mode) if ssl_mode else ""

    cfg: Dict[str, Optional[str]] = {
        "username": username,
        "password": password,
        "host": host,
        "port": str(port),
        "database": database,
        "ssl_mode": ssl_mode,
        "ssl_ca": ssl_ca or None,
    }

    # Sensible defaults for TiDB Cloud hosts if SSL mode not provided
    try:
        if cfg["host"] and cfg["host"].endswith(".tidbcloud.com") and not cfg["ssl_mode"]:
            cfg["ssl_mode"] = "VERIFY_IDENTITY"
            # macOS system certs bundle path (LibreSSL/OpenSSL shim)
            if os.path.exists("/etc/ssl/cert.pem"):
                cfg["ssl_ca"] = cfg["ssl_ca"] or "/etc/ssl/cert.pem"
    except Exception:
        pass

    return cfg


def build_mysql_command(mysql_bin: str, cfg: Dict[str, Optional[str]], sql: str) -> Tuple[list[str], Dict[str, str]]:
    cmd = [mysql_bin, "--protocol=TCP", "-h", cfg["host"] or "127.0.0.1", "-P", cfg["port"] or "4000"]
    if cfg.get("username"):
        cmd += ["-u", cfg["username"]]  # type: ignore[arg-type]
    if cfg.get("database"):
        cmd += ["-D", cfg["database"]]  # type: ignore[arg-type]
    # Help some clients pick the correct auth by default
    cmd.append("--default-auth=mysql_native_password")
    if cfg.get("ssl_mode"):
        cmd.append(f"--ssl-mode={cfg['ssl_mode']}")
    if cfg.get("ssl_ca"):
        cmd.append(f"--ssl-ca={cfg['ssl_ca']}")

    env = os.environ.copy()
    if cfg.get("password"):
        env["MYSQL_PWD"] = cfg["password"]  # type: ignore[index]

    cmd += ["-e", sql]
    return cmd, env


def redacted_command_preview(cmd: list[str]) -> str:
    # Redact any env password usage is already in env; here just return the shell-escaped command
    return " ".join(shlex.quote(part) for part in cmd)


def main() -> int:
    repo_root = os.path.dirname(os.path.abspath(__file__))
    load_dotenv_into_environ(os.path.join(repo_root, ".env"))

    url = os.environ.get("DATABASE_URL")
    if not url:
        print("Error: DATABASE_URL is not set. Define it in .env or environment.", file=sys.stderr)
        print(
            "Example: DATABASE_URL=mysql://user:password@127.0.0.1:4000/dbname?ssl-mode=DISABLED",
            file=sys.stderr,
        )
        return 1

    mysql_bin = find_mysql_binary()
    if not mysql_bin:
        print(
            "Error: mysql client not found. Install with Homebrew: brew install mysql-client",
            file=sys.stderr,
        )
        print("If already installed, ensure it is on PATH or located at /opt/homebrew/bin/mysql.", file=sys.stderr)
        return 2

    cfg = parse_database_url(url)

    system_schemas = {
        "mysql",
        "INFORMATION_SCHEMA",
        "PERFORMANCE_SCHEMA",
        "METRICS_SCHEMA",
        "INSPECTION_SCHEMA",
        "sys",
    }

    if cfg.get("database"):
        sql = (
            "SELECT table_schema, table_name "
            "FROM information_schema.tables "
            f"WHERE table_schema = '{cfg['database']}' "
            "ORDER BY table_schema, table_name;"
        )
    else:
        excluded = ",".join(f"'{s}'" for s in sorted(system_schemas))
        sql = (
            "SELECT table_schema, table_name "
            "FROM information_schema.tables "
            f"WHERE table_schema NOT IN ({excluded}) "
            "ORDER BY table_schema, table_name;"
        )

    cmd, env = build_mysql_command(mysql_bin, cfg, sql)

    # Ensure plugin dir is set to mysql-client's plugins if available
    plugin_dir = env.get("MYSQL_PLUGIN_DIR") or find_mysql_plugin_dir()
    if plugin_dir and "MYSQL_PLUGIN_DIR" not in env:
        env["MYSQL_PLUGIN_DIR"] = plugin_dir

    print("Connecting with:")
    # Show safe preview (no password printed)
    print("  ", redacted_command_preview(cmd))
    # Warn if MariaDB client is detected (auth plugin incompatibilities with TiDB)
    try:
        ver = subprocess.run([mysql_bin, "--version"], capture_output=True, text=True, check=False)
        out = (ver.stdout or ver.stderr or "").strip()
        if "MariaDB" in out:
            print(
                "Warning: Detected MariaDB client. Prefer Oracle MySQL client to avoid auth plugin issues.",
                file=sys.stderr,
            )
            print(
                "Install via Homebrew: brew install mysql-client && export PATH=\"/opt/homebrew/opt/mysql-client/bin:$PATH\"",
                file=sys.stderr,
            )
    except Exception:
        pass
    if cfg.get("ssl_mode"):
        print(f"  SSL mode: {cfg['ssl_mode']}")
    if cfg.get("ssl_ca"):
        print(f"  SSL CA: {cfg['ssl_ca']}")

    try:
        result = subprocess.run(cmd, env=env, check=False)
        if result.returncode != 0:
            print("\nConnection or query failed (non-zero exit).", file=sys.stderr)
            print("Troubleshooting:", file=sys.stderr)
            print("  - Verify TiDB is reachable (default port 4000).", file=sys.stderr)
            print("  - Ensure user, password, and database are correct.", file=sys.stderr)
            print("  - If TLS issues occur, try adding ?ssl-mode=DISABLED to DATABASE_URL.", file=sys.stderr)
            print("  - If using TLS with a custom CA, set ?ssl-mode=VERIFY_CA&ssl-ca=/path/to/ca.pem.", file=sys.stderr)
            print(
                "  - If you see an auth plugin error, install Oracle MySQL client: brew install mysql-client",
                file=sys.stderr,
            )
            print(
                "    And ensure PATH uses /opt/homebrew/opt/mysql-client/bin before other mysql binaries.",
                file=sys.stderr,
            )
        return result.returncode
    except FileNotFoundError:
        print("Error: mysql client not found at runtime.", file=sys.stderr)
        return 127
    except Exception as e:  # pragma: no cover
        print(f"Error running mysql: {e}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    sys.exit(main())


