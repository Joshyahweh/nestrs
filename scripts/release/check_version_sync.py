#!/usr/bin/env python3
import pathlib
import re
import sys


SEMVER_RE = re.compile(r"^\d+\.\d+\.\d+$")
CHANGELOG_VERSION_RE = re.compile(r"^## \[(?P<version>\d+\.\d+\.\d+)\]")


def read_text(path: pathlib.Path) -> str:
    return path.read_text(encoding="utf-8").strip()


def extract_latest_released_version(changelog: str) -> str | None:
    for line in changelog.splitlines():
        match = CHANGELOG_VERSION_RE.match(line.strip())
        if match:
            return match.group("version")
    return None


def main() -> int:
    root = pathlib.Path(__file__).resolve().parents[2]
    version_path = root / "VERSION"
    changelog_path = root / "CHANGELOG.md"

    if not version_path.exists():
        print(f"[release-check] missing VERSION file at {version_path}")
        return 1
    if not changelog_path.exists():
        print(f"[release-check] missing CHANGELOG.md at {changelog_path}")
        return 1

    version = read_text(version_path)
    changelog = read_text(changelog_path)

    if not SEMVER_RE.match(version):
        print(f"[release-check] VERSION must be plain semver (X.Y.Z), got: {version}")
        return 1

    if "## [Unreleased]" not in changelog:
        print("[release-check] CHANGELOG.md must contain an '## [Unreleased]' section")
        return 1

    latest_released = extract_latest_released_version(changelog)
    if latest_released is None:
        print("[release-check] CHANGELOG.md must contain at least one released version heading")
        return 1

    if latest_released != version:
        print(
            "[release-check] VERSION and latest CHANGELOG release mismatch: "
            f"VERSION={version}, latest_changelog={latest_released}"
        )
        return 1

    print(
        "[release-check] release metadata is consistent: "
        f"VERSION={version}, latest_changelog={latest_released}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
