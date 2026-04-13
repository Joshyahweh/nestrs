---
title: "Authorization"
description: "What this page covers: Authorization in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Authorization** in the **security** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **security** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="security" topic="authorization" title="Authorization" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs authorization
```

## Notes and pitfalls

<Hint>
Separate authentication and authorization so policies remain auditable.
</Hint>

<Warning>
Never rely on client-provided roles without server-side verification.
</Warning>
