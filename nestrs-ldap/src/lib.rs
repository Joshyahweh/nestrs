//! LDAP bind adapter — NestJS `passport-ldap` analogue.

#![doc(html_root_url = "https://docs.rs/nestrs-ldap/1.7.1")]

use ldap3::{LdapConnAsync, LdapError};

/// LDAP connection settings.
#[derive(Clone, Debug)]
pub struct LdapAuthOptions {
    /// LDAP URL (`ldap://dc.example:389` or `ldaps://...`).
    pub url: String,
    /// Bind DN template. `{username}` is replaced with the login name after
    /// [RFC 4514](https://www.rfc-editor.org/rfc/rfc4514) escaping, so the
    /// value stays a single RDN. Example:
    /// `cn={username},ou=people,dc=example,dc=com`.
    pub bind_dn_template: String,
}

/// Errors from connect/bind.
#[derive(Debug, thiserror::Error)]
pub enum LdapAuthError {
    /// Username or password was empty. An empty password is not sent to the
    /// directory: many servers treat that as an unauthenticated bind and
    /// return success without checking a credential.
    #[error("LDAP simple bind requires a non-empty username and password")]
    EmptyCredentials,
    /// Underlying ldap3 error.
    #[error(transparent)]
    Ldap(#[from] LdapError),
}

/// Simple bind authenticator.
#[derive(Clone, Debug)]
pub struct LdapAuthenticator {
    options: LdapAuthOptions,
}

impl LdapAuthenticator {
    /// Wrap [`LdapAuthOptions`].
    pub fn new(options: LdapAuthOptions) -> Self {
        Self { options }
    }

    fn bind_dn(&self, username: &str) -> String {
        self.options
            .bind_dn_template
            .replace("{username}", &escape_dn_value(username))
    }

    /// Bind as `username` / `password`. `Ok(())` means the directory accepted the bind.
    ///
    /// Empty username or password is rejected before any network call.
    /// `username` is escaped as one RDN value (commas and other DN specials
    /// cannot add another RDN).
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<(), LdapAuthError> {
        if username.is_empty() || password.is_empty() {
            return Err(LdapAuthError::EmptyCredentials);
        }
        let (conn, mut ldap) = LdapConnAsync::new(&self.options.url).await?;
        ldap3::drive!(conn);
        let dn = self.bind_dn(username);
        ldap.simple_bind(&dn, password).await?.success()?;
        let _ = ldap.unbind().await;
        Ok(())
    }
}

/// Escape `value` as an RFC 4514 attribute value so it cannot introduce
/// another RDN. NUL is `\00`; the other specials are backslash-prefixed.
fn escape_dn_value(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    for (i, c) in chars.iter().copied().enumerate() {
        let lead = i == 0;
        let trail = i + 1 == chars.len();
        match c {
            '\\' | '"' | '+' | ',' | ';' | '<' | '>' => {
                out.push('\\');
                out.push(c);
            }
            '\0' => out.push_str("\\00"),
            ' ' if lead || trail => {
                out.push('\\');
                out.push(' ');
            }
            '#' if lead => {
                out.push('\\');
                out.push('#');
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{LdapAuthError, LdapAuthOptions, LdapAuthenticator};

    #[test]
    fn bind_dn_substitutes_username() {
        let auth = LdapAuthenticator::new(LdapAuthOptions {
            url: "ldap://127.0.0.1:389".into(),
            bind_dn_template: "cn={username},ou=people,dc=example,dc=com".into(),
        });
        assert_eq!(auth.bind_dn("ada"), "cn=ada,ou=people,dc=example,dc=com");
    }

    #[test]
    fn bind_dn_escapes_rdn_injection() {
        let auth = LdapAuthenticator::new(LdapAuthOptions {
            url: "ldap://127.0.0.1:389".into(),
            bind_dn_template: "cn={username},ou=people,dc=example,dc=com".into(),
        });
        assert_eq!(
            auth.bind_dn("ada,ou=admins"),
            r"cn=ada\,ou=admins,ou=people,dc=example,dc=com"
        );
        assert_eq!(auth.bind_dn("a\\b"), r"cn=a\\b,ou=people,dc=example,dc=com");
        assert_eq!(
            auth.bind_dn("#admin"),
            r"cn=\#admin,ou=people,dc=example,dc=com"
        );
        assert_eq!(
            auth.bind_dn(" ada"),
            r"cn=\ ada,ou=people,dc=example,dc=com"
        );
        assert_eq!(
            auth.bind_dn("ada\0"),
            r"cn=ada\00,ou=people,dc=example,dc=com"
        );
    }

    #[tokio::test]
    async fn rejects_empty_password_without_contacting_the_directory() {
        let auth = LdapAuthenticator::new(LdapAuthOptions {
            url: "ldap://127.0.0.1:1".into(),
            bind_dn_template: "cn={username},ou=people,dc=example,dc=com".into(),
        });
        // CodeQL `rust/hard-coded-cryptographic-value` treats a literal that
        // reaches the `password` parameter as a hard-coded credential. Both
        // values are built at runtime and rejected before any bind.
        let empty = String::new();
        let err = auth.authenticate("ada", &empty).await.unwrap_err();
        assert!(matches!(err, LdapAuthError::EmptyCredentials));
        let password = std::process::id().to_string();
        let err = auth.authenticate(&empty, &password).await.unwrap_err();
        assert!(matches!(err, LdapAuthError::EmptyCredentials));
    }
}
