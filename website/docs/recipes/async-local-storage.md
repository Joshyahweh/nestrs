---
title: "Async local storage"
description: "What this page covers: Async local storage in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Async local storage** in the **recipes** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **recipes** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="recipes" topic="async-local-storage" title="Async local storage" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs async_local_storage
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
