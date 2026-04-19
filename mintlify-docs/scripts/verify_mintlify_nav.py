#!/usr/bin/env python3
"""
Fail if docs.json lists a navigation page with no matching .mdx file.

Run from the repository root:
  python3 mintlify-docs/scripts/verify_mintlify_nav.py
"""
from __future__ import annotations

import json
import pathlib
import sys

MINTLIFY_ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS_JSON = MINTLIFY_ROOT / "docs.json"


def _page_slugs(data: object) -> list[str]:
    out: list[str] = []
    nav = data if isinstance(data, dict) else {}
    for tab in nav.get("navigation", {}).get("tabs", []):
        if not isinstance(tab, dict):
            continue
        for group in tab.get("groups", []):
            if not isinstance(group, dict):
                continue
            for page in group.get("pages", []):
                if isinstance(page, str):
                    out.append(page)
    return out


def main() -> int:
    if not DOCS_JSON.is_file():
        print(f"error: {DOCS_JSON} not found", file=sys.stderr)
        return 1
    data = json.loads(DOCS_JSON.read_text(encoding="utf-8"))
    missing: list[str] = []
    for page in _page_slugs(data):
        mdx = MINTLIFY_ROOT / f"{page}.mdx"
        if not mdx.is_file():
            try:
                rel = mdx.relative_to(MINTLIFY_ROOT.parent)
            except ValueError:
                rel = mdx
            missing.append(str(rel))
    if missing:
        print(
            "docs.json lists pages with no matching .mdx under mintlify-docs/:",
            file=sys.stderr,
        )
        for m in missing:
            print(f"  - {m}", file=sys.stderr)
        return 1
    print("OK: all Mintlify navigation pages have a .mdx file")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
