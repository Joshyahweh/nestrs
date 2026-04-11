use crate::core::{ModuleGraph, ProviderRegistry};
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, Uri};
use axum::response::Response;
use std::marker::PhantomData;
use std::sync::Arc;
use tower::ServiceExt;

pub struct TestingModule {
    registry: Arc<ProviderRegistry>,
    router: axum::Router,
}

impl TestingModule {
    pub fn builder<M>() -> TestingModuleBuilder<M>
    where
        M: ModuleGraph,
    {
        TestingModuleBuilder::new()
    }

    pub fn get<T>(&self) -> Arc<T>
    where
        T: Send + Sync + 'static,
    {
        self.registry.get::<T>()
    }

    pub fn http_client(&self) -> TestClient {
        TestClient {
            router: self.router.clone(),
        }
    }
}

pub struct TestingModuleBuilder<M>
where
    M: ModuleGraph,
{
    overrides: Vec<Box<dyn FnOnce(&mut ProviderRegistry) + Send>>,
    configure_http: Option<Box<dyn FnOnce(crate::NestApplication) -> crate::NestApplication + Send>>,
    _marker: PhantomData<M>,
}

impl<M> TestingModuleBuilder<M>
where
    M: ModuleGraph,
{
    pub fn new() -> Self {
        Self {
            overrides: Vec::new(),
            configure_http: None,
            _marker: PhantomData,
        }
    }

    pub fn override_provider<T>(mut self, instance: Arc<T>) -> Self
    where
        T: crate::core::Injectable + Send + Sync + 'static,
    {
        self.overrides
            .push(Box::new(move |registry| registry.override_provider::<T>(instance)));
        self
    }

    pub fn configure_http<F>(mut self, f: F) -> Self
    where
        F: FnOnce(crate::NestApplication) -> crate::NestApplication + Send + 'static,
    {
        self.configure_http = Some(Box::new(f));
        self
    }

    pub async fn compile(self) -> TestingModule {
        let mut registry = ProviderRegistry::new();
        M::register_providers(&mut registry);

        for o in self.overrides {
            o(&mut registry);
        }

        let router = M::register_controllers(axum::Router::new(), &registry);

        let registry = Arc::new(registry);
        let mut app = crate::NestApplication {
            registry: registry.clone(),
            router,
            uri_version: None,
            global_prefix: None,
            static_mounts: Vec::new(),
            cors_options: None,
            security_headers: None,
            rate_limit_options: None,
            request_timeout: None,
            concurrency_limit: None,
            load_shed: false,
            body_limit_bytes: None,
            production_errors: false,
            request_id: false,
            request_context: false,
            request_scope: false,
            i18n: false,
            liveness_path: None,
            readiness: None,
            metrics_path: None,
            #[cfg(feature = "openapi")]
            openapi: None,
            request_tracing: None,
            global_layers: Vec::new(),
            exception_filter: None,
            default_404_fallback: false,
            compression: false,
            request_decompression: false,
            listen_ip: None,
            path_normalization: None,
        };

        if let Some(cfg) = self.configure_http {
            app = cfg(app);
        }

        let built_registry = app.registry.clone();
        let built_router = app.into_router();

        TestingModule {
            registry: built_registry,
            router: built_router,
        }
    }
}

#[derive(Clone)]
pub struct TestClient {
    router: axum::Router,
}

impl TestClient {
    pub fn request(&self, method: Method, uri: impl AsRef<str>) -> TestRequest {
        TestRequest {
            router: self.router.clone(),
            method,
            uri: uri.as_ref().to_string(),
            headers: HeaderMap::new(),
            body: Body::empty(),
        }
    }

    pub fn get(&self, uri: impl AsRef<str>) -> TestRequest {
        self.request(Method::GET, uri)
    }

    pub fn post(&self, uri: impl AsRef<str>) -> TestRequest {
        self.request(Method::POST, uri)
    }
}

pub struct TestRequest {
    router: axum::Router,
    method: Method,
    uri: String,
    headers: HeaderMap,
    body: Body,
}

impl TestRequest {
    pub fn header(mut self, name: axum::http::HeaderName, value: axum::http::HeaderValue) -> Self {
        self.headers.insert(name, value);
        self
    }

    pub fn body(mut self, body: Body) -> Self {
        self.body = body;
        self
    }

    pub fn json<T>(self, value: &T) -> Self
    where
        T: serde::Serialize,
    {
        let body = crate::serde_json::to_vec(value).expect("serialize json body");
        self.header(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        )
        .body(Body::from(body))
    }

    pub async fn send(self) -> Response {
        let uri: Uri = self.uri.parse().expect("valid uri");
        let mut builder = Request::builder().method(self.method).uri(uri);

        for (k, v) in self.headers.iter() {
            builder = builder.header(k, v);
        }

        let req = builder.body(self.body).expect("build request");
        self.router
            .oneshot(req)
            .await
            .expect("router oneshot")
    }
}

