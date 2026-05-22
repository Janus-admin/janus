use axum::{
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use include_dir::{include_dir, Dir};

static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/dashboard/out");

pub async fn serve(uri: axum::http::Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');

    let candidates: Vec<String> = if raw.is_empty() {
        vec!["index.html".to_string()]
    } else {
        vec![
            raw.to_string(),
            format!("{}/index.html", raw.trim_end_matches('/')),
            format!("{}.html", raw.trim_end_matches('/')),
        ]
    };

    for candidate in &candidates {
        if let Some(file) = ASSETS.get_file(candidate.as_str()) {
            let ct = content_type(candidate);
            let cache = if candidate.contains("/_next/static/") {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            };
            let mut resp = (StatusCode::OK, file.contents()).into_response();
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
            return resp;
        }
    }

    // fallback: serve 404.html with 404 status
    if let Some(file) = ASSETS.get_file("404.html") {
        let mut resp = (StatusCode::NOT_FOUND, file.contents()).into_response();
        resp.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        return resp;
    }

    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
