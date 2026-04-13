---
title: "Workspaces"
description: "What this page covers: Workspaces in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Workspaces** in the **cli** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **cli** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="cli" topic="workspaces" title="Workspaces" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-cli workspaces
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
