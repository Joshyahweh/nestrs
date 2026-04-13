---
title: "Sentry"
description: "What this page covers: Sentry in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Sentry** in the **recipes** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **recipes** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="recipes" topic="sentry" title="Sentry" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs sentry
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
