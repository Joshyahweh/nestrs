# nestrs-ldap

LDAP simple-bind adapter (NestJS `passport-ldap` analogue).

```toml
nestrs-ldap = "1.7.1"
```

```rust,ignore
let auth = nestrs_ldap::LdapAuthenticator::new(nestrs_ldap::LdapAuthOptions {
    url: "ldap://dc.example:389".into(),
    bind_dn_template: "cn={username},ou=people,dc=example,dc=com".into(),
});
auth.authenticate("ada", "s3cret").await?;
```

`authenticate` rejects an empty username or password before connecting, so a directory that treats an empty password as an anonymous bind cannot succeed the call. `{username}` is escaped as one DN value (RFC 4514): a comma in the login name cannot add another RDN.
