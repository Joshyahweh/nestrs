---
title: "Security"
description: "What this page covers: Security in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Security** in the **security** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **security** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="security" topic="security" title="Security" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs security
```

## Notes and pitfalls

<Hint>
Separate authentication and authorization so policies remain auditable.
</Hint>

<Warning>
Never rely on client-provided roles without server-side verification.
</Warning>
