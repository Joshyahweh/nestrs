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
#[derive(Clone)]
pub enum KafkaSaslOptions {
    Plain { username: String, password: String },
    ScramSha256 { username: String, password: String },
    ScramSha512 { username: String, password: String },
}

impl std::fmt::Debug for KafkaSaslOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Passwords are never rendered — Debug output flows into logs, panic
        // messages, and error reports. The mechanism + username stay
        // visible for operators.
        let (mechanism, username) = match self {
            KafkaSaslOptions::Plain { username, .. } => ("Plain", username),
            KafkaSaslOptions::ScramSha256 { username, .. } => ("ScramSha256", username),
            KafkaSaslOptions::ScramSha512 { username, .. } => ("ScramSha512", username),
        };
        f.debug_struct(mechanism)
            .field("username", username)
            .field("password", &"<redacted>")
            .finish()
    }
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
        use rustls::pki_types::pem::PemObject;
        for item in rustls::pki_types::CertificateDer::pem_slice_iter(pem.as_bytes()) {
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

#[cfg(test)]
mod redaction_tests {
    use super::*;

    #[test]
    fn sasl_debug_never_shows_the_password() {
        let opts = KafkaSaslOptions::Plain {
            username: "alice".to_string(),
            password: "hunter2-DO-NOT-LOG".to_string(),
        };
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("hunter2"), "password leaked: {rendered}");
        assert!(rendered.contains("<redacted>"), "no redaction marker: {rendered}");
        assert!(rendered.contains("alice"), "username should stay visible: {rendered}");

        let opts = KafkaSaslOptions::ScramSha512 {
            username: "bob".to_string(),
            password: "topsecret".to_string(),
        };
        let rendered = format!("{opts:?}");
        assert!(!rendered.contains("topsecret"), "password leaked: {rendered}");
        assert!(rendered.contains("ScramSha512"));

        // Composition: a whole options tree with nested SASL stays clean.
        let conn = KafkaConnectionOptions {
            sasl: Some(KafkaSaslOptions::ScramSha256 {
                username: "carol".to_string(),
                password: "s3cr3t".to_string(),
            }),
            ..Default::default()
        };
        let rendered = format!("{conn:?}");
        assert!(!rendered.contains("s3cr3t"), "password leaked via parent: {rendered}");
    }
}
