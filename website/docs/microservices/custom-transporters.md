---
title: "Custom transporters"
description: "What this page covers: Custom transporters in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Custom transporters** in the **microservices** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **microservices** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="microservices" topic="custom-transporters" title="Custom transporters" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-microservices custom_transporters
```

## Notes and pitfalls

<Hint>
Model examples around message patterns and transport wiring, not HTTP controllers.
</Hint>

<Warning>
Transport retries and idempotency are required for at-least-once delivery semantics.
</Warning>
