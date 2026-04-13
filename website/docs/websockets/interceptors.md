---
title: "Interceptors"
description: "What this page covers: Interceptors in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Interceptors** in the **websockets** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **websockets** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="websockets" topic="interceptors" title="Interceptors" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-ws interceptors
```

## Notes and pitfalls

<Hint>
Use gateway-level concerns (connection, events, filters) instead of route handlers.
</Hint>

<Warning>
Do not trust client event payloads; validate and sanitize every inbound message.
</Warning>
