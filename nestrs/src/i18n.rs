use crate::core::{DynamicModule, Injectable, ProviderRegistry};
use crate::module;
use axum::extract::Request;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::any::TypeId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct I18nOptions {
    pub fallback_locale: String,
    pub supported_locales: Option<Vec<String>>,
    /// Query parameter used to override locale (default: `"lang"`). Set to `None` to disable.
    pub query_param: Option<String>,
    /// Catalogs: locale -> (key -> message).
    pub catalogs: HashMap<String, HashMap<String, String>>,
}

impl Default for I18nOptions {
    fn default() -> Self {
        Self {
            fallback_locale: "en".to_string(),
            supported_locales: None,
            query_param: Some("lang".to_string()),
            catalogs: HashMap::new(),
        }
    }
}

impl I18nOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_fallback_locale(mut self, locale: impl Into<String>) -> Self {
        self.fallback_locale = locale.into();
        self
    }

    pub fn with_supported_locales<I, S>(mut self, locales: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.supported_locales = Some(locales.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_query_param(mut self, name: Option<impl Into<String>>) -> Self {
        self.query_param = name.map(Into::into);
        self
    }

    pub fn insert(&mut self, locale: impl Into<String>, key: impl Into<String>, value: impl Into<String>) {
        let locale = locale.into();
        self.catalogs
            .entry(locale)
            .or_default()
            .insert(key.into(), value.into());
    }
}

fn normalize_locale(raw: &str) -> String {
    raw.trim()
        .replace('_', "-")
        .to_ascii_lowercase()
}

fn base_locale(locale: &str) -> &str {
    locale.split('-').next().unwrap_or(locale)
}

fn query_get<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    // Locale values are typically ASCII (`en`, `en-US`, etc), so we keep this parser lightweight:
    // no percent-decoding, no repeated key aggregation.
    for part in query.split('&') {
        let mut it = part.splitn(2, '=');
        let k = it.next()?.trim();
        if k != key {
            continue;
        }
        let v = it.next().unwrap_or("").trim();
        if v.is_empty() {
            return None;
        }
        return Some(v);
    }
    None
}

fn parse_accept_language(value: &str) -> Option<String> {
    let mut best: Option<(String, f32)> = None;
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut it = part.split(';');
        let lang = it.next().unwrap_or("").trim();
        if lang.is_empty() || lang == "*" {
            continue;
        }
        let mut q = 1.0_f32;
        for param in it {
            let param = param.trim();
            if let Some(v) = param.strip_prefix("q=") {
                q = v.parse::<f32>().unwrap_or(0.0);
            }
        }

        match best {
            None => best = Some((lang.to_string(), q)),
            Some((_, best_q)) if q > best_q => best = Some((lang.to_string(), q)),
            _ => {}
        }
    }
    best.map(|(l, _)| l)
}

pub struct I18nService {
    fallback_locale: String,
    supported_locales: Option<HashSet<String>>,
    query_param: Option<String>,
    catalogs: HashMap<String, HashMap<String, String>>,
}

#[nestrs::async_trait]
impl Injectable for I18nService {
    fn construct(_registry: &ProviderRegistry) -> Arc<Self> {
        Arc::new(Self::from_options(I18nOptions::default()))
    }
}

impl I18nService {
    pub fn from_options(mut options: I18nOptions) -> Self {
        // Normalize locale keys so resolution is consistent.
        let mut catalogs = HashMap::<String, HashMap<String, String>>::new();
        for (loc, map) in options.catalogs.drain() {
            catalogs.insert(normalize_locale(&loc), map);
        }

        let supported_locales = options.supported_locales.map(|v| {
            v.into_iter()
                .map(|s| normalize_locale(&s))
                .collect::<HashSet<_>>()
        });

        Self {
            fallback_locale: normalize_locale(&options.fallback_locale),
            supported_locales,
            query_param: options.query_param,
            catalogs,
        }
    }

    pub fn resolve_locale(&self, headers: &HeaderMap, uri: &Uri) -> String {
        // 1) Query override (`?lang=fr`)
        if let (Some(param), Some(q)) = (self.query_param.as_deref(), uri.query()) {
            if let Some(v) = query_get(q, param) {
                return self.pick_supported(normalize_locale(v));
            }
        }

        // 2) Accept-Language
        if let Some(raw) = headers
            .get(axum::http::header::ACCEPT_LANGUAGE)
            .and_then(|v| v.to_str().ok())
        {
            if let Some(lang) = parse_accept_language(raw) {
                return self.pick_supported(normalize_locale(&lang));
            }
        }

        // 3) Fallback
        self.fallback_locale.clone()
    }

    fn pick_supported(&self, locale: String) -> String {
        let Some(supported) = &self.supported_locales else {
            return locale;
        };
        if supported.contains(&locale) {
            return locale;
        }
        let base = normalize_locale(base_locale(&locale));
        if supported.contains(&base) {
            return base;
        }
        self.fallback_locale.clone()
    }

    pub fn t(&self, locale: &str, key: &str) -> String {
        self.t_with(locale, key, &[])
    }

    pub fn t_with(&self, locale: &str, key: &str, vars: &[(&str, &str)]) -> String {
        let locale = normalize_locale(locale);
        let base = normalize_locale(base_locale(&locale));

        let raw = self
            .catalogs
            .get(&locale)
            .and_then(|m| m.get(key))
            .or_else(|| self.catalogs.get(&base).and_then(|m| m.get(key)))
            .or_else(|| {
                self.catalogs
                    .get(&self.fallback_locale)
                    .and_then(|m| m.get(key))
            })
            .map(|s| s.as_str())
            .unwrap_or(key);

        let mut out = raw.to_string();
        for (k, v) in vars {
            let needle = format!("{{{k}}}");
            out = out.replace(&needle, v);
        }
        out
    }
}

#[derive(Clone, Debug)]
pub struct Locale(pub String);

/// Returned when [`Locale`] / [`I18n`] extractors are used but [`crate::NestApplication::use_i18n`] was not enabled.
#[derive(Debug)]
pub struct I18nMissing;

impl IntoResponse for I18nMissing {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "nestrs: Locale/I18n extractor requires NestApplication::use_i18n()",
        )
            .into_response()
    }
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for Locale
where
    S: Send + Sync,
{
    type Rejection = I18nMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Locale>().cloned().ok_or(I18nMissing)
    }
}

#[derive(Clone)]
pub struct I18n {
    pub locale: String,
    service: Arc<I18nService>,
}

impl I18n {
    pub fn t(&self, key: &str) -> String {
        self.service.t(&self.locale, key)
    }

    pub fn t_with(&self, key: &str, vars: &[(&str, &str)]) -> String {
        self.service.t_with(&self.locale, key, vars)
    }

    pub fn service(&self) -> Arc<I18nService> {
        self.service.clone()
    }
}

#[async_trait::async_trait]
impl<S> axum::extract::FromRequestParts<S> for I18n
where
    S: Send + Sync,
{
    type Rejection = I18nMissing;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<I18n>().cloned().ok_or(I18nMissing)
    }
}

pub(crate) async fn install_i18n_middleware(
    axum::extract::State(i18n): axum::extract::State<Arc<I18nService>>,
    req: Request,
    next: Next,
) -> Response {
    let (mut parts, body) = req.into_parts();
    let locale = i18n.resolve_locale(&parts.headers, &parts.uri);
    parts.extensions.insert(Locale(locale.clone()));
    parts.extensions.insert(I18n {
        locale,
        service: i18n,
    });
    let req = Request::from_parts(parts, body);
    next.run(req).await
}

#[module(providers = [I18nService], exports = [I18nService])]
pub struct I18nModule;

impl I18nModule {
    pub fn register(options: I18nOptions) -> DynamicModule {
        let mut registry = ProviderRegistry::new();
        registry.override_provider::<I18nService>(Arc::new(I18nService::from_options(options)));
        DynamicModule::from_parts(
            registry,
            axum::Router::new(),
            vec![TypeId::of::<I18nService>()],
        )
    }
}

