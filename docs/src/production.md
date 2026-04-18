# Production runbook

Treat this chapter as the **operations companion** to [Observability](observability.md) and [Secure defaults](secure-defaults.md): the embedded **`PRODUCTION_RUNBOOK.md`** goes deep on deployment, benchmarking, fuzzing, and storage—content that would go stale if duplicated here.

**API examples:** Operational toggles such as **`enable_metrics`**, **`enable_health_check`**, and **`configure_tracing`** appear with copy-paste snippets in the [API cookbook](appendix-api-cookbook.md).

## Before you deploy

- **Tracing**: Configure a single subscriber ([Observability](observability.md)); avoid ad-hoc `println!` in hot paths.  
- **Metrics**: Expose `/metrics` (or your chosen path) and exclude it from noisy access logs via `RequestTracingOptions::skip_paths`.  
- **Secrets**: Load from the environment or a secrets manager; do not bake production tokens into container images.  
- **TLS**: Terminate at the edge (load balancer, ingress) or in-process—document which tier owns certificates.  
- **Backpressure**: Combine application limits (`use_body_limit`, `use_rate_limit`, timeouts) with edge protections.  

## Where the full runbook helps

The included file covers **benchmark operations**, **libFuzzer** smoke workflows, and **benchmark storage** playbooks that change with CI—link to the file in PRs when you touch those areas.

---

{{#include ../../PRODUCTION_RUNBOOK.md}}

