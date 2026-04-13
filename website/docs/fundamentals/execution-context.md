---
title: "Execution context"
description: "What this page covers: Execution context in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Execution context** in the **fundamentals** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **fundamentals** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="fundamentals" topic="execution-context" title="Execution context" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-core execution_context
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
