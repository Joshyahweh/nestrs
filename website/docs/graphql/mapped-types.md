---
title: "Mapped types"
description: "What this page covers: Mapped types in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Mapped types** in the **graphql** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **graphql** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="graphql" topic="mapped-types" title="Mapped types" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-graphql mapped_types
```

## Notes and pitfalls

<Hint>
Focus on resolvers, schema types, and field-level behavior rather than REST endpoints.
</Hint>

<Warning>
Unbounded query complexity can degrade performance; enforce complexity limits.
</Warning>
