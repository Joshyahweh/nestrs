---
title: "Types and Parameters"
description: "What this page covers: Types and Parameters in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Types and Parameters** in the **openapi** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **openapi** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="openapi" topic="types-and-parameters" title="Types and Parameters" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-openapi types_and_parameters
```

## Notes and pitfalls

<Hint>
Keep generated contracts synchronized with runtime guards and DTO validation.
</Hint>

<Warning>
Outdated schemas cause integration regressions; regenerate contracts in CI.
</Warning>
