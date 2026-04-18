# Release

Maintainers and crate consumers use **`RELEASE.md`** for versioning steps, publish checks, and coordination with **`CHANGELOG.md`**. The full procedure is included below.

## Readers’ guide

- **Application developers**: You usually only need the **semver** story in `STABILITY.md` and the **changelog**—[Changelog](changelog.md).  
- **Maintainers / releasers**: Follow **`RELEASE.md`** end to end; ensure CI (security, benchmarks as applicable) is green on the release commit.  
- **Downstream packagers**: Prefer **crates.io** versions; git dependencies should pin revisions intentionally.  

---

{{#include ../../RELEASE.md}}

