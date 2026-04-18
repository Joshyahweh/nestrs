# Changelog

The canonical history of user-visible changes is **`CHANGELOG.md`** at the repository root (included below). It follows **Keep a Changelog**-style sections; version numbers align with workspace crates and **`VERSION`**.

## How to use this with semver

- **API stability** guarantees and `#[doc(hidden)]` policy are documented in **`STABILITY.md`** (repo root)—read it when upgrading across minor versions.  
- **Breaking changes** should be called out explicitly in the changelog section for that release.  
- If you depend on **internal** modules or features, pin versions and watch for `#[doc(hidden)]` churn in rustdoc.  

---

{{#include ../../CHANGELOG.md}}

