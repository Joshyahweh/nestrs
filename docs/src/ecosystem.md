# Ecosystem modules

Optional crates and feature flags extend nestrs with **cache**, **scheduled jobs**, **queues**, and **i18n**—roughly where Nest users reach for `@nestjs/cache-manager`, `@nestjs/schedule`, Bull queues, and `nestjs-i18n`. Everything here is **explicit**: enable the right **Cargo features**, import the matching **`DynamicModule`**, and register providers/controllers like any other module.

## When to use which module

| Need | Module | Typical trigger |
|------|--------|-----------------|
| Reduce load on databases or upstream APIs | **CacheModule** | Repeated reads of the same key; optional Redis for multi-instance deployments. |
| Cron or interval work inside the process | **ScheduleModule** | Replace OS cron for app-local tasks; not a distributed scheduler by itself. |
| Background work with a baseline in-process queue | **QueuesModule** | Feature `queues`; swap for an external broker when you need durability across nodes. |
| Locale-aware responses | **I18nModule** | Call `NestApplication::use_i18n()` and plug catalogs/resolvers as documented in the include below. |

## Cargo features (checklist)

Enable features on the **`nestrs`** dependency in your `Cargo.toml` (exact feature names match your workspace version—see the main crate on [docs.rs](https://docs.rs/nestrs)). Common pairs:

- **Caching**: `cache` and optionally `cache-redis` for the Redis backend.  
- **Scheduling**: `schedule`.  
- **Queues**: `queues`.  
- **i18n**: follow the crate’s feature flags in the embedded ecosystem document.  

If something fails at link time or with “feature not enabled” errors, compare your **`Cargo.toml`** with the [CLI](cli.md) **`nestrs doctor`** output and the crate READMEs under `nestrs-*` in the repo.

**`NestApplication` helpers:** [`use_i18n`](https://docs.rs/nestrs/latest/nestrs/struct.NestApplication.html#method.use_i18n) is documented in rustdoc; see also the [API cookbook](appendix-api-cookbook.md) for related app-wide methods.

---

{{#include ../../ECOSYSTEM.md}}

