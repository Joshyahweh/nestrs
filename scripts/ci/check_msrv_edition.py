#!/usr/bin/env python3
"""Verify workspace MSRV and edition policy."""

from __future__ import annotations

import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKSPACE_MANIFEST = ROOT / "Cargo.toml"
EXPECTED_MSRV = "1.88"
EXPECTED_EDITION = "2021"


def load_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text())


def main() -> int:
    root = load_toml(WORKSPACE_MANIFEST)
    workspace_package = root.get("workspace", {}).get("package", {})
    msrv = workspace_package.get("rust-version")
    edition = workspace_package.get("edition")

    errors: list[str] = []
    if msrv != EXPECTED_MSRV:
        errors.append(
            f"workspace.package.rust-version must be {EXPECTED_MSRV!r}, got {msrv!r}"
        )
    if edition != EXPECTED_EDITION:
        errors.append(
            f"workspace.package.edition must be {EXPECTED_EDITION!r}, got {edition!r}"
        )

    members = root.get("workspace", {}).get("members", [])
    for member in members:
        manifest = ROOT / member / "Cargo.toml"
        if not manifest.exists():
            errors.append(f"missing member manifest: {manifest}")
            continue

        data = load_toml(manifest)
        package = data.get("package", {})
        if not package:
            continue

        edition_workspace = package.get("edition") == {"workspace": True}
        rust_version_workspace = package.get("rust-version") == {"workspace": True}

        if not edition_workspace:
            errors.append(
                f"{member}/Cargo.toml should inherit edition from workspace "
                "(set edition = { workspace = true })"
            )
        if not rust_version_workspace:
            errors.append(
                f"{member}/Cargo.toml should inherit rust-version from workspace "
                "(set rust-version = { workspace = true })"
            )

    if errors:
        print("MSRV/edition policy check failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print(
        "MSRV/edition policy check passed "
        f"(edition={EXPECTED_EDITION}, rust-version={EXPECTED_MSRV})."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
