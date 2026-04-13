---
title: "Hybrid application"
description: "What this page covers: Hybrid application in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Hybrid application** in the **faq** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **faq** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="faq" topic="hybrid-application" title="Hybrid application" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs hybrid_application
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
