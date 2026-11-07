use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::BytesRejection;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use stravia_core::Gateway;
use stravia_core::platform_plugins::PlatformPluginUpdate;

const MAX_COMPONENT_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(
            "/platform-plugins",
            get(list)
                .post(install)
                .layer(DefaultBodyLimit::max(MAX_COMPONENT_BYTES)),
        )
        .route("/platform-plugins/{id}/settings", put(update))
        .route("/platform-plugins/{id}", delete(uninstall))
        .layer(middleware::from_fn(no_store))
}

async fn list(State(gateway): State<Gateway>) -> Response {
    match gateway.admin().list_platform_plugins().await {
        Ok(plugins) => success(plugins),
        Err(error) => failure(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()),
    }
}

async fn install(
    State(gateway): State<Gateway>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Response {
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_none_or(|value| !value.eq_ignore_ascii_case("application/wasm"))
    {
        return failure(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Content-Type must be application/wasm",
        );
    }
    let body = match body {
        Ok(body) => body,
        Err(rejection) => return rejection.into_response(),
    };
    if body.is_empty() {
        return failure(
            StatusCode::BAD_REQUEST,
            "plugin component body must not be empty",
        );
    }
    match gateway.admin().install_platform_plugin(body.to_vec()).await {
        Ok(plugin) => success(plugin),
        Err(error) => failure(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn update(
    State(gateway): State<Gateway>,
    Path(id): Path<String>,
    Json(input): Json<PlatformPluginUpdate>,
) -> Response {
    match gateway.admin().update_platform_plugin(&id, input).await {
        Ok(plugin) => success(plugin),
        Err(error) => failure(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

async fn uninstall(State(gateway): State<Gateway>, Path(id): Path<String>) -> Response {
    match gateway.admin().uninstall_platform_plugin(&id).await {
        Ok(()) => success(()),
        Err(error) => failure(StatusCode::BAD_REQUEST, error.to_string()),
    }
}

fn success<T: serde::Serialize>(data: T) -> Response {
    Json(serde_json::json!({ "data": data })).into_response()
}

fn failure(status: StatusCode, message: impl Into<String>) -> Response {
    (status, Json(serde_json::json!({ "error": message.into() }))).into_response()
}

async fn no_store(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
