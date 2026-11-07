//! NestJS [`@nestjs/outbox`](https://docs.nestjs.com/techniques/outbox) analogue —
//! transactional outbox, relay, retries, and dead-letter queue.
//!
//! # Flow
//!
//! 1. [`OutboxRelay::add`] inside the same logical transaction as business writes.
//! 2. Background loop calls [`OutboxRelay::tick`] with a [`PublishFn`].
//! 3. Success → published; failure → retry; after `max_attempts` → DLQ.
//!
//! # Module
//!
//! ```ignore
//! use nestrs_outbox::{OutboxModule, OutboxOptions};
//! let dm = OutboxModule::register(OutboxOptions::default());
//! ```
//!
//! Set `allow_in_memory: false` once a durable store adapter is configured.
//! Pair with `nestrs-webhooks` for signed outbound delivery.
//!
//! Umbrella feature: `nestrs` / `outbox` → `nestrs::outbox`.

#![doc(html_root_url = "https://docs.rs/nestrs-outbox/1.7.1")]

mod module;
mod options;
mod relay;
mod store;

pub use module::OutboxModule;
pub use options::OutboxOptions;
pub use relay::{OutboxRelay, PublishFn};
pub use store::{
    DeadLetter, InMemoryOutboxStore, OutboxMessage, OutboxStatus, OutboxStore, SharedOutboxStore,
};

pub use nestrs_core::{DynamicModule, Injectable, ProviderRegistry};
