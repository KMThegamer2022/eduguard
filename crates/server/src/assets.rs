//! Console assets, embedded into the server binary so a single file deploys.

use axum::http::header;
use axum::response::{IntoResponse, Response};

pub fn index() -> Response {
    html(include_str!("../console/index.html"))
}

pub fn app_js() -> Response {
    (
        [(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
        include_str!("../console/app.js"),
    )
        .into_response()
}

pub fn style_css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../console/style.css"),
    )
        .into_response()
}

pub fn favicon() -> Response {
    (
        [(header::CONTENT_TYPE, "image/svg+xml")],
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#3fb950" stroke-width="2"><path d="M12 2l8 4v6c0 5-3.5 8.5-8 10-4.5-1.5-8-5-8-10V6l8-4z"/><path d="M9 12l2 2 4-4"/></svg>"##,
    )
        .into_response()
}

fn html(body: &'static str) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}
