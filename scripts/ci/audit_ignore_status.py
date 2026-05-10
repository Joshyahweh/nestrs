#!/usr/bin/env python3
"""Report whether ignored RustSec advisories are still needed."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
AUDIT_CONFIG = ROOT / ".cargo" / "audit.toml"
LOCKFILE = ROOT / "Cargo.lock"


def load_ignored_ids() -> list[str]:
    data = tomllib.loads(AUDIT_CONFIG.read_text())
    advisories = data.get("advisories", {})
    ignores = advisories.get("ignore", [])
    if not isinstance(ignores, list):
        raise ValueError("Expected [advisories].ignore to be a list")
    return [str(item) for item in ignores]


def run_audit_without_repo_config() -> dict:
    cmd = ["cargo", "audit", "--json", "--file", str(LOCKFILE)]
    with tempfile.TemporaryDirectory() as tmp:
        proc = subprocess.run(
            cmd,
            cwd=tmp,
            text=True,
            capture_output=True,
            check=False,
        )
    # cargo-audit may return non-zero because yanked checks timed out, while still producing
    # a complete JSON report on stdout. Use the report when possible.
    try:
        report = json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        sys.stderr.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        raise RuntimeError("cargo audit did not produce valid JSON output") from exc

    if proc.returncode != 0 and proc.stderr.strip():
        print(
            "note: cargo audit exited non-zero (often registry/yanked lookup issues), "
            "but JSON report was available and used.",
            file=sys.stderr,
        )
        print(proc.stderr.strip(), file=sys.stderr)

    return report


def collect_vulnerability_ids(report: dict) -> set[str]:
    vulns = report.get("vulnerabilities", {}).get("list", [])
    ids: set[str] = set()
    for item in vulns:
        advisory = item.get("advisory", {})
        advisory_id = advisory.get("id")
        if advisory_id:
            ids.add(advisory_id)
    return ids


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fail-on-removable",
        action="store_true",
        help="Exit non-zero when an ignored advisory is no longer reported.",
    )
    args = parser.parse_args()

    ignored = load_ignored_ids()
    report = run_audit_without_repo_config()
    active_vuln_ids = collect_vulnerability_ids(report)

    still_needed = [advisory for advisory in ignored if advisory in active_vuln_ids]
    removable = [advisory for advisory in ignored if advisory not in active_vuln_ids]

    print("Audit ignore review")
    print(f"- ignored IDs in .cargo/audit.toml: {len(ignored)}")
    print(f"- active vulnerability IDs (without repo ignores): {len(active_vuln_ids)}")
    print("")

    if still_needed:
        print("Still needed ignores:")
        for advisory in still_needed:
            print(f"- {advisory}")
    else:
        print("Still needed ignores: none")

    print("")
    if removable:
        print("Potentially removable ignores:")
        for advisory in removable:
            print(f"- {advisory}")
    else:
        print("Potentially removable ignores: none")

    if args.fail_on_removable and removable:
        print("")
        print("One or more ignores are removable. Please update .cargo/audit.toml.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
