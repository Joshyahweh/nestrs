#!/usr/bin/env python3
"""Generate and verify the public-API snapshot for the nestrs workspace.

`cargo public-api` reads `rustdoc --output-format json`, which is nightly-only.
Rather than depend on an external install, this script does the same job
inline: runs `cargo +nightly rustdoc --output-format json -Z unstable-options`
for each target crate, reads the resulting JSON's `paths` table (canonical
fully-qualified paths for every public item), filters out `#[doc(hidden)]`
items, and writes a sorted list to `tests/api-snapshots/<crate>.txt`.

CI gate: regenerate, diff against committed snapshots, fail on drift.

Public API policy (see STABILITY.md):
- Include items visible at the crate's lib root or any pub module under it.
- Exclude `#[doc(hidden)]` items — not part of the stable API.
- Include nested items (struct fields, trait methods, enum variants) via
  rustdoc's `paths` table.

Format: one fully-qualified path per line, sorted, no header.

Why this exists (instead of using cargo-public-api directly):
cargo-public-api requires installing a third-party tool from crates.io,
which isn't always feasible in CI environments with restricted network or
the auto-mode classifier. rustdoc JSON is stable on nightly; we read the
same fields cargo-public-api reads (the `paths` table, the `index`'s
`attrs` for doc(hidden) detection) and produce equivalent output.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SNAPSHOT_DIR = REPO_ROOT / "tests" / "api-snapshots"

# Crates the snapshot covers. Matches STABILITY.md "primarily" list plus
# the extension crates named in the same paragraph. nestrs-macros is
# excluded: proc-macro symbol surface is unstable across rustc versions
# and not what users depend on. nestrs-oauth2 was added when its
# `authorization-server` feature landed: published at 1.0.0, a large
# security-sensitive public surface, and no snapshot meant no drift gate
# at all. The remaining published extension crates (nestrs-openapi,
# nestrs-microservices, nestrs-storage, ...) are a deliberate gap for a
# future decision, not an oversight.
TARGET_CRATES = [
    "nestrs",
    "nestrs-core",
    "nestrs-graphql",
    "nestrs-ws",
    "nestrs-mcp",
    "nestrs-oauth2",
]


def run_rustdoc_json(crate: str) -> dict:
    """Run `cargo +nightly rustdoc --output-format json` for one crate."""
    cmd = [
        "cargo", "+nightly", "rustdoc",
        "--lib",
        "-p", crate,
        "--all-features",
        "--output-format", "json",
        "-Z", "unstable-options",
    ]
    proc = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(
            f"rustdoc failed for {crate} (exit {proc.returncode}):\n"
            f"  stdout: {proc.stdout[-2000:]}\n"
            f"  stderr: {proc.stderr[-2000:]}\n"
        )
        raise SystemExit(proc.returncode)
    # Cargo writes to target/doc/<crate_name>.json (hyphen → underscore).
    json_path = REPO_ROOT / "target" / "doc" / f"{crate.replace('-', '_')}.json"
    if not json_path.exists():
        sys.stderr.write(f"expected {json_path} but it does not exist\n")
        raise SystemExit(2)
    with json_path.open() as f:
        return json.load(f)


def is_doc_hidden(item_id: str, index: dict) -> bool:
    """STABILITY.md policy: `#[doc(hidden)]` is not part of the stable API."""
    item = index.get(item_id)
    if item is None:
        return False
    for attr in item.get("attrs", []) or []:
        if isinstance(attr, str) and "doc(hidden)" in attr:
            return True
    return False


def is_internal_helper(path: str) -> bool:
    """STABILITY.md policy: `__nestrs_*` are macro-internal helpers.

    They're explicitly named as out-of-scope even when they lack
    `#[doc(hidden)]` — they're a name prefix reserved for macro codegen
    and can be renamed / re-shaped in any release.
    """
    return "::__nestrs_" in path or path.startswith("__nestrs_")


def local_crate_id(doc: dict) -> int:
    """rustdoc convention: the local crate is `external_crates[0]` (id 0).

    We cross-check: any `crate_id` in `paths` that is NOT in
    `external_crates` is the local crate. In practice this is always 0.
    """
    ec = doc.get("external_crates", {})
    ec_ids = {int(k) for k in ec.keys()}
    path_ids = {v["crate_id"] for v in doc["paths"].values()}
    candidates = path_ids - ec_ids
    if len(candidates) != 1:
        sys.stderr.write(
            f"WARNING: expected exactly one local crate id, found {candidates}\n"
        )
    return next(iter(candidates))


def iter_public_items(doc: dict) -> list[str]:
    """Walk the public item tree from the crate root, return sorted paths."""
    index: dict = doc["index"]
    paths: dict = doc["paths"]
    local_id = local_crate_id(doc)
    items: set[str] = set()
    for item_id, entry in paths.items():
        if entry["crate_id"] != local_id:
            continue
        if is_doc_hidden(item_id, index):
            continue
        path = "::".join(entry["path"])
        if is_internal_helper(path):
            continue
        items.add(path)
    return sorted(items)


def write_snapshot(crate: str, items: list[str]) -> Path:
    SNAPSHOT_DIR.mkdir(parents=True, exist_ok=True)
    out_path = SNAPSHOT_DIR / f"{crate}.txt"
    out_path.write_text("\n".join(items) + ("\n" if items else ""))
    return out_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="regenerate and diff against committed snapshots, exit 1 on drift",
    )
    parser.add_argument(
        "--crate",
        help="only regenerate / check one crate (default: all)",
    )
    args = parser.parse_args()

    targets = [args.crate] if args.crate else TARGET_CRATES
    drift: list[str] = []

    for crate in targets:
        doc = run_rustdoc_json(crate)
        items = iter_public_items(doc)
        if args.check:
            committed = SNAPSHOT_DIR / f"{crate}.txt"
            current = "\n".join(items) + ("\n" if items else "")
            committed_text = committed.read_text() if committed.exists() else ""
            if current != committed_text:
                cur_set = set(current.splitlines())
                com_set = set(committed_text.splitlines())
                added = sorted(cur_set - com_set)
                removed = sorted(com_set - cur_set)
                drift.append(crate)
                sys.stderr.write(f"\n=== drift in {crate} ===\n")
                if added:
                    sys.stderr.write("  added:\n")
                    for line in added[:20]:
                        sys.stderr.write(f"    + {line}\n")
                    if len(added) > 20:
                        sys.stderr.write(f"    + ... ({len(added) - 20} more)\n")
                if removed:
                    sys.stderr.write("  removed:\n")
                    for line in removed[:20]:
                        sys.stderr.write(f"    - {line}\n")
                    if len(removed) > 20:
                        sys.stderr.write(f"    - ... ({len(removed) - 20} more)\n")
            else:
                sys.stderr.write(f"  {crate}: OK ({len(items)} items)\n")
        else:
            out = write_snapshot(crate, items)
            sys.stderr.write(
                f"  wrote {out.relative_to(REPO_ROOT)} ({len(items)} items)\n"
            )

    if args.check and drift:
        sys.stderr.write(
            f"\nFAIL: {len(drift)} crate(s) drifted from committed snapshot.\n"
            "  Run `python3 scripts/ci/public_api_snapshot.py` locally to "
            "regenerate, then commit the updated files.\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
