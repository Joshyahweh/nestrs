---
title: "Encryption and Hashing"
description: "What this page covers: Encryption and Hashing in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Encryption and Hashing** in the **security** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **security** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="security" topic="encryption-and-hashing" title="Encryption and Hashing" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs encryption_and_hashing
```

## Notes and pitfalls

<Hint>
Separate authentication and authorization so policies remain auditable.
</Hint>

<Warning>
Never rely on client-provided roles without server-side verification.
</Warning>
