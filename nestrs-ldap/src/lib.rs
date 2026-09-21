//! LDAP bind adapter — NestJS `passport-ldap` analogue.

#![doc(html_root_url = "https://docs.rs/nestrs-ldap/1.4.0")]

use ldap3::{LdapConnAsync, LdapError};

/// LDAP connection settings.
#[derive(Clone, Debug)]
pub struct LdapAuthOptions {
    /// LDAP URL (`ldap://dc.example:389` or `ldaps://...`).
    pub url: String,
    /// Bind DN template. `{username}` is replaced with the login name.
    /// Example: `cn={username},ou=people,dc=example,dc=com`.
    pub bind_dn_template: String,
}

/// Errors from connect/bind.
#[derive(Debug, thiserror::Error)]
pub enum LdapAuthError {
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
            .replace("{username}", username)
    }

    /// Bind as `username` / `password`. `Ok(())` means the directory accepted the bind.
    pub async fn authenticate(&self, username: &str, password: &str) -> Result<(), LdapAuthError> {
        let (conn, mut ldap) = LdapConnAsync::new(&self.options.url).await?;
        ldap3::drive!(conn);
        let dn = self.bind_dn(username);
        ldap.simple_bind(&dn, password).await?.success()?;
        let _ = ldap.unbind().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{LdapAuthOptions, LdapAuthenticator};

    #[test]
    fn bind_dn_substitutes_username() {
        let auth = LdapAuthenticator::new(LdapAuthOptions {
            url: "ldap://127.0.0.1:389".into(),
            bind_dn_template: "cn={username},ou=people,dc=example,dc=com".into(),
        });
        assert_eq!(auth.bind_dn("ada"), "cn=ada,ou=people,dc=example,dc=com");
    }
}
