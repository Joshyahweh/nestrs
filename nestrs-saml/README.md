# nestrs-saml

SAML 2.0 service-provider adapter (NestJS `passport-saml` analogue).

Builds the IdP redirect; you validate `SAMLResponse` via
`SamlResponseValidator` (samael, xmlsec, or your IdP SDK).

```toml
nestrs-saml = "1.5.0"
```
