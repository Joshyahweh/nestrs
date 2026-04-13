---
title: "Standalone apps"
description: "What this page covers: Standalone apps in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Standalone apps** in the **deployment** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **deployment** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="deployment" topic="standalone-apps" title="Standalone apps" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs standalone_apps
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
