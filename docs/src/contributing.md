# Contributing

Thank you for helping improve nestrs. The **normative** contribution guide (branching, checks, review expectations) is **`CONTRIBUTING.md`** at the repository root and is included below so this book stays aligned with GitHub’s default contributor path.

## Before your first PR

1. **Build and test**: `cargo check --workspace` and `cargo test --workspace` from the repo root.  
2. **Scope**: Keep changes focused; update **tests** when behavior or public contracts change.  
3. **Docs**: If you change user-visible behavior, add or adjust **mdBook** pages under `docs/src/` or **rustdoc** on public types.  
4. **ADRs**: If you change an architectural decision captured in [Architecture decisions](adrs.md), update or add an ADR in the same PR.  
5. **Conduct**: Follow **`CODE_OF_CONDUCT.md`** in the repository root.  

---

{{#include ../../CONTRIBUTING.md}}

