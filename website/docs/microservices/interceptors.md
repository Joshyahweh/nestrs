---
title: "Interceptors"
description: "What this page covers: Interceptors in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Interceptors** in the **microservices** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **microservices** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="microservices" topic="interceptors" title="Interceptors" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-microservices interceptors
```

## Notes and pitfalls

<Hint>
Model examples around message patterns and transport wiring, not HTTP controllers.
</Hint>

<Warning>
Transport retries and idempotency are required for at-least-once delivery semantics.
</Warning>
