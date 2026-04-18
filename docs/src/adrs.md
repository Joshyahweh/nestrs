# Architecture decisions (ADRs)

These ADRs capture long-lived architectural choices that affect contributors and reviewers. Each ADR is a short **decision record**: context, decision, consequences—see the linked files for full text.

| ADR | Summary |
|-----|---------|
| [0001 — Axum-only HTTP engine](adrs/0001-axum-only.md) | Core HTTP stays **Axum**; other protocols layer on top instead of adding a second HTTP backend. |
| [0002 — Process-global registries](adrs/0002-global-registries.md) | Route and metadata registries are **process-global**; test resets are **feature-gated** (`test-hooks`). |
| [0003 — Macro expansion strategy](adrs/0003-macro-expansion-strategy.md) | Proc macros favor **explicit**, reviewable expansion over hidden runtime magic. |

## When to add or change an ADR

- **Add** a new ADR when a decision is **stable**, **cross-cutting**, and would otherwise live only in chat or a long PR thread.  
- **Supersede** an ADR (new file with a link from the old one) when the decision is reversed or replaced—do not silently rewrite history.  
- **Link** from the PR description so reviewers see the rationale alongside the code.  

When a contribution changes one of these assumptions, update or supersede the relevant ADR in the same pull request.
