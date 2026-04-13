#!/usr/bin/env python3
from __future__ import annotations

import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SIDEBAR_CONFIG = ROOT / "lib" / "sidebar.config.ts"
DOCS_ROOT = ROOT / "docs"


def parse_sidebar_items(config_text: str) -> list[tuple[str, str]]:
    pattern = re.compile(r'item\("([^"]+)",\s*"([^"]+)"\)')
    return pattern.findall(config_text)


def to_snake(value: str) -> str:
    value = re.sub(r"[^a-zA-Z0-9]+", "_", value)
    value = re.sub(r"_+", "_", value).strip("_").lower()
    return value or "example"


CLI_PACKAGE_BY_SECTION = {
    "introduction": "nestrs",
    "fundamentals": "nestrs-core",
    "techniques": "nestrs",
    "security": "nestrs",
    "graphql": "nestrs-graphql",
    "websockets": "nestrs-ws",
    "microservices": "nestrs-microservices",
    "deployment": "nestrs",
    "cli": "nestrs-cli",
    "openapi": "nestrs-openapi",
    "recipes": "nestrs",
    "faq": "nestrs",
    "devtools": "nestrs-cli",
    "discover": "nestrs",
}


HINT_BY_SECTION = {
    "microservices": "Model examples around message patterns and transport wiring, not HTTP controllers.",
    "websockets": "Use gateway-level concerns (connection, events, filters) instead of route handlers.",
    "graphql": "Focus on resolvers, schema types, and field-level behavior rather than REST endpoints.",
    "security": "Separate authentication and authorization so policies remain auditable.",
    "openapi": "Keep generated contracts synchronized with runtime guards and DTO validation.",
}


WARNING_BY_SECTION = {
    "microservices": "Transport retries and idempotency are required for at-least-once delivery semantics.",
    "websockets": "Do not trust client event payloads; validate and sanitize every inbound message.",
    "graphql": "Unbounded query complexity can degrade performance; enforce complexity limits.",
    "security": "Never rely on client-provided roles without server-side verification.",
    "openapi": "Outdated schemas cause integration regressions; regenerate contracts in CI.",
}


def build_markdown(title: str, slug: str) -> str:
    section = slug.split("/")[0]
    topic = slug.split("/")[-1]
    snake = to_snake(topic)
    package = CLI_PACKAGE_BY_SECTION.get(section, "nestrs")
    hint = HINT_BY_SECTION.get(
        section,
        "Keep examples focused on the core abstraction for this page so intent stays clear.",
    )
    warning = WARNING_BY_SECTION.get(
        section,
        "Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.",
    )

    return f"""---
title: "{title}"
description: "What this page covers: {title} in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **{title}** in the **{section}** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **{section}** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="{section}" topic="{topic}" title="{title}" />

## CLI check

```sh filename="terminal"
$ cargo test -p {package} {snake}
```

## Notes and pitfalls

<Hint>
{hint}
</Hint>

<Warning>
{warning}
</Warning>
"""


def main() -> None:
    config_text = SIDEBAR_CONFIG.read_text(encoding="utf-8")
    items = parse_sidebar_items(config_text)
    written = 0

    for title, slug in items:
        out_file = DOCS_ROOT / f"{slug}.md"
        out_file.parent.mkdir(parents=True, exist_ok=True)
        out_file.write_text(build_markdown(title, slug), encoding="utf-8")
        written += 1

    print(f"Generated {written} markdown files from sidebar config.")


if __name__ == "__main__":
    main()
