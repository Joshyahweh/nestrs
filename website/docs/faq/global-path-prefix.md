---
title: "Global path prefix"
description: "What this page covers: Global path prefix in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Global path prefix** in the **faq** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **faq** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="faq" topic="global-path-prefix" title="Global path prefix" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs global_path_prefix
```

## Notes and pitfalls

<Hint>
Keep examples focused on the core abstraction for this page so intent stays clear.
</Hint>

<Warning>
Avoid copy-pasting examples blindly; adapt to your module boundaries and runtime constraints.
</Warning>
