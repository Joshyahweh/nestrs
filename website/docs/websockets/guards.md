---
title: "Guards"
description: "What this page covers: Guards in nestrs, with section-specific practical examples."
---

## Why this matters

This page documents **Guards** in the **websockets** section and shows how to apply the right abstraction for this domain.

<Info>
Examples on this page are intentionally scoped to **websockets** concerns so they stay aligned with the architecture.
</Info>

## Implementation sample

<AutoCodeTabs section="websockets" topic="guards" title="Guards" />

## CLI check

```sh filename="terminal"
$ cargo test -p nestrs-ws guards
```

## Notes and pitfalls

<Hint>
Use gateway-level concerns (connection, events, filters) instead of route handlers.
</Hint>

<Warning>
Do not trust client event payloads; validate and sanitize every inbound message.
</Warning>
