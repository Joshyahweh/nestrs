//! SAML 2.0 **service provider** adapter — NestJS `passport-saml` analogue.
//!
//! Builds an SP-initiated redirect to the IdP (`AuthnRequest`). Assertion
//! consumption (crypto, XML signature) stays in your app or a dedicated
//! XML stack; plug [`SamlResponseValidator`] at the ACS route.

#![doc(html_root_url = "https://docs.rs/nestrs-saml/1.3.0")]

use url::Url;
use uuid::Uuid;

/// Service-provider settings (entity id, ACS, IdP SSO URL).
#[derive(Clone, Debug)]
pub struct SamlServiceProvider {
    /// SP entity ID.
    pub entity_id: String,
    /// Assertion Consumer Service URL.
    pub acs_url: String,
    /// Identity provider HTTP-Redirect SSO URL.
    pub idp_sso_url: String,
}

impl SamlServiceProvider {
    /// SP-initiated login URL (HTTP-Redirect binding).
    ///
    /// `SAMLRequest` is a deflated-and-base64 AuthnRequest in a full IdP
    /// integration; this adapter sends a **relay-friendly** request id and
    /// metadata the ACS can correlate. Pair with [`SamlResponseValidator`]
    /// on POST `/acs`.
    pub fn redirect_url(&self, relay_state: &str) -> Result<Url, url::ParseError> {
        let mut url = Url::parse(&self.idp_sso_url)?;
        let request_id = format!("id-{}", Uuid::new_v4());
        url.query_pairs_mut()
            .append_pair("SAMLRequest", &request_id)
            .append_pair("RelayState", relay_state)
            .append_pair("spEntityId", &self.entity_id)
            .append_pair("acs", &self.acs_url);
        Ok(url)
    }
}

/// Consume a SAMLResponse at the ACS. Implement this in the app with
/// `samael`, `xmlsec`, or a hosted IdP SDK.
pub trait SamlResponseValidator: Send + Sync {
    /// The error type returned when an assertion cannot be accepted.
    type Error;
    /// Validate a raw `SAMLResponse` POST body / form field.
    fn validate(&self, saml_response: &str) -> Result<SamlAssertion, Self::Error>;
}

/// Minimal assertion view after a successful ACS.
#[derive(Clone, Debug)]
pub struct SamlAssertion {
    /// NameID / subject.
    pub name_id: String,
    /// RelayState echoed from the AuthnRequest, if any.
    pub relay_state: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::SamlServiceProvider;

    #[test]
    fn redirect_url_carries_acs_and_entity() {
        let sp = SamlServiceProvider {
            entity_id: "https://app.example/sp".into(),
            acs_url: "https://app.example/acs".into(),
            idp_sso_url: "https://idp.example/sso".into(),
        };
        let url = sp.redirect_url("after-login").unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            q.get("acs").map(String::as_str),
            Some("https://app.example/acs")
        );
        assert_eq!(q.get("RelayState").map(String::as_str), Some("after-login"));
        assert!(q.contains_key("SAMLRequest"));
    }
}
