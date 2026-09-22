# nestrs-bullmq

BullMQ-compatible Redis producer for nestrs. Use this when Node `@nestjs/bullmq`
workers (or other BullMQ consumers) must share the same Redis keys.

nestrs `QueuesModule` + `queues-redis` stays the in-process / LPUSH-BRPOP path.

```toml
nestrs-bullmq = "1.5.0"
```

```rust,ignore
let producer = nestrs_bullmq::BullMqProducer::new("redis://127.0.0.1/", "email")?;
producer.add("send", &serde_json::json!({"to": "a@b.c"})).await?;
```
