//! Spring Security's default response headers, which Komga's security chains add to every
//! `/api/**`, `/opds/**` and `/sse/**` response. `nosniff` matters most for
//! `/api/v1/books/{id}/resource/**`, which serves files from inside EPUBs on the same origin.

use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

const HEADERS: [(HeaderName, &str); 3] = [
    (axum::http::header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (axum::http::header::X_FRAME_OPTIONS, "SAMEORIGIN"),
    (axum::http::header::X_XSS_PROTECTION, "0"),
];

fn applies(path: &str) -> bool {
    ["/api/", "/opds/", "/sse/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
}

pub async fn security_headers_middleware(request: Request, next: Next) -> Response {
    let applies = applies(request.uri().path());
    let mut response = next.run(request).await;
    if applies {
        let headers = response.headers_mut();
        for (name, value) in HEADERS {
            headers.insert(name, HeaderValue::from_static(value));
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn api_opds_and_sse_get_the_headers_and_the_web_ui_does_not() {
        let app = Router::new()
            .fallback(get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(security_headers_middleware));
        for (path, expected) in [
            ("/api/v1/books", true),
            ("/opds/v1.2/catalog", true),
            ("/sse/v1/events", true),
            ("/", false),
            ("/series/s1", false),
        ] {
            let response = app
                .clone()
                .oneshot(axum::http::Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            let headers = response.headers();
            for (name, value) in HEADERS {
                assert_eq!(
                    headers.get(&name).map(|v| v.to_str().unwrap()),
                    expected.then_some(value),
                    "{path} {name}"
                );
            }
        }
    }
}
