---
title: "Decorators"
description: "What this page covers: Decorators in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Decorators** in the **openapi** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **openapi** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="openapi" topic="decorators" title="Decorators" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-openapi decorators
```

## Notes and pitfalls

<Hint>
Keep generated contracts synchronized with runtime guards and DTO validation.
</Hint>

<Warning>
Outdated schemas cause integration regressions; regenerate contracts in CI.
</Warning>
