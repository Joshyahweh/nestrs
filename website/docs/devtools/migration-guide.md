---
title: "Migration guide"
description: "What this page covers: Migration guide in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Migration guide** in the **devtools** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **devtools** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="devtools" topic="migration-guide" title="Migration guide" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-cli migration_guide
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
