# nestrs-ldap

LDAP simple-bind adapter (NestJS `passport-ldap` analogue).

```toml
nestrs-ldap = "1.3.0"
```

```rust,ignore
let auth = nestrs_ldap::LdapAuthenticator::new(nestrs_ldap::LdapAuthOptions {
    url: "ldap://dc.example:389".into(),
    bind_dn_template: "cn={username},ou=people,dc=example,dc=com".into(),
});
auth.authenticate("ada", "s3cret").await?;
```
