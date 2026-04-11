//! Kafka broker connection (TLS + SASL) helpers for [rskafka](https://docs.rs/rskafka).

use std::sync::Arc;

use rskafka::client::{ClientBuilder, Credentials, SaslConfig};
use rustls::RootCertStore;

/// Optional TLS for Kafka (TLS is recommended when using SASL over the public internet).
#[derive(Clone, Debug, Default)]
pub struct KafkaTlsOptions {
    /// PEM-encoded CA certificate(s). When `None`, [webpki_roots](https://docs.rs/webpki-roots) is used.
    pub ca_cert_pem: Option<String>,
}

/// SASL authentication (maps to rskafka [`SaslConfig`]).
#[derive(Clone, Debug)]
pub enum KafkaSaslOptions {
    Plain { username: String, password: String },
    ScramSha256 { username: String, password: String },
    ScramSha512 { username: String, password: String },
}

/// Shared broker connection knobs for [`super::KafkaTransportOptions`] and [`super::KafkaMicroserviceOptions`].
#[derive(Clone, Debug, Default)]
pub struct KafkaConnectionOptions {
    pub client_id: Option<String>,
    pub tls: Option<KafkaTlsOptions>,
    pub sasl: Option<KafkaSaslOptions>,
}

impl KafkaSaslOptions {
    fn to_rskafka(&self) -> SaslConfig {
        match self {
            KafkaSaslOptions::Plain { username, password } => {
                SaslConfig::Plain(Credentials::new(username.clone(), password.clone()))
            }
            KafkaSaslOptions::ScramSha256 { username, password } => {
                SaslConfig::ScramSha256(Credentials::new(username.clone(), password.clone()))
            }
            KafkaSaslOptions::ScramSha512 { username, password } => {
                SaslConfig::ScramSha512(Credentials::new(username.clone(), password.clone()))
            }
        }
    }
}

fn build_rustls_config(opts: &KafkaTlsOptions) -> Result<Arc<rustls::ClientConfig>, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut root_store = RootCertStore::empty();
    if let Some(pem) = opts.ca_cert_pem.as_deref() {
        let mut cursor = std::io::Cursor::new(pem.as_bytes());
        for item in rustls_pemfile::certs(&mut cursor) {
            let cert = item.map_err(|e| format!("PEM parse: {e}"))?;
            root_store
                .add(cert)
                .map_err(|e| format!("bad CA cert: {e}"))?;
        }
    } else {
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(Arc::new(root_store))
        .with_no_client_auth();

    Ok(Arc::new(config))
}

/// Start from bootstrap brokers and apply TLS / SASL / client id.
pub(crate) fn client_builder_from_parts(
    bootstrap_brokers: Vec<String>,
    conn: &KafkaConnectionOptions,
) -> Result<ClientBuilder, String> {
    let mut b = ClientBuilder::new(bootstrap_brokers);
    if let Some(id) = conn.client_id.as_deref() {
        b = b.client_id(Arc::from(id));
    }
    if let Some(sasl) = &conn.sasl {
        b = b.sasl_config(sasl.to_rskafka());
    }
    if let Some(tls) = &conn.tls {
        let cfg = build_rustls_config(tls)?;
        b = b.tls_config(cfg);
    }
    Ok(b)
}
