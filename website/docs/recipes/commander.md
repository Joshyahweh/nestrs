---
title: "Commander"
description: "What this page covers: Commander in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Commander** in the **recipes** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **recipes** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="recipes" topic="commander" title="Commander" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs commander
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
