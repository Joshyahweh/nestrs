use axum::body::Body;
use axum::http::{Request, StatusCode};
use nestrs::prelude::*;
use tower::util::ServiceExt;

#[derive(Default)]
#[injectable]
struct PostsService;

impl PostsService {
    fn label(&self) -> &'static str {
        "posts"
    }
}

#[derive(Default)]
#[injectable]
struct UsersService;

impl UsersService {
    fn label(&self) -> &'static str {
        "users"
    }
}

#[controller(prefix = "/one")]
struct OneController;

#[routes(state = PostsService)]
impl OneController {
    #[get("/")]
    async fn show(posts: std::sync::Arc<PostsService>) -> &'static str {
        posts.label()
    }
}

#[controller(prefix = "/combo")]
struct ComboController;

#[routes(state = (PostsService, UsersService))]
impl ComboController {
    #[get("/")]
    async fn both(
        posts: std::sync::Arc<PostsService>,
        users: std::sync::Arc<UsersService>,
    ) -> String {
        format!("{}-{}", posts.label(), users.label())
    }

    #[get("/posts")]
    async fn posts_only(State(posts): State<std::sync::Arc<PostsService>>) -> &'static str {
        posts.label()
    }

    #[get("/plain")]
    async fn plain() -> &'static str {
        "plain"
    }
}

#[module(
    controllers = [OneController, ComboController],
    providers = [PostsService, UsersService],
)]
struct AppModule;

async fn body_of(router: axum::Router, uri: &str) -> (StatusCode, String) {
    let response = router
        .oneshot(
            Request::builder()
                .uri(uri)
                .method("GET")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("serve");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 64)
        .await
        .expect("body");
    (status, String::from_utf8(bytes.to_vec()).expect("utf8"))
}

#[tokio::test]
async fn route_state_can_be_taken_by_arc_or_composed() {
    let router = NestFactory::create::<AppModule>().into_router();

    let (status, body) = body_of(router.clone(), "/one").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "posts");

    let (status, body) = body_of(router.clone(), "/combo").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "posts-users");

    let (status, body) = body_of(router.clone(), "/combo/posts").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "posts");

    let (status, body) = body_of(router, "/combo/plain").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "plain");
}
