# nestrs-passport

`@nestjs/passport` analogue: strategies on nestrs [`AuthStrategy`], not Node
`passport`. Use with `AuthStrategyGuard` / `PassportGuard`.

```toml
nestrs-passport = "1.3.0"
```

```rust,ignore
use nestrs_passport::{JwtStrategy, PassportGuard};

let strategy = JwtStrategy::new(|token| async move { Ok(token) });
// register as a provider and `#[use_guards(PassportGuard::<JwtStrategy<_>>)]`
```

- **`JwtStrategy`** — `Authorization: Bearer`
- **`LocalBasicStrategy`** — `Authorization: Basic` (body-based local login
  still belongs in a handler; `AuthStrategy` only sees request parts)
