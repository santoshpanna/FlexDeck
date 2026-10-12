//! Serving the portal's own files.
//!
//! The cloud is one deployable: this crate is its logic, `apps/portal` is its
//! interface, and the two are served from the same origin. That is not a
//! convenience — it is what makes the session cookie work. A cookie set by the
//! API is only sent back to the same site, so a portal on a different origin
//! would need CORS *and* a cross-site cookie policy, which is a much larger
//! surface than serving both from here.
//!
//! # Why the fallback is `index.html` and not a 404
//!
//! The portal is a single-page app that owns its own routing. A deep link like
//! `/desktops/abc` is a route the *app* understands, so the server must hand it
//! the app rather than deciding it does not exist. This is the same reasoning the
//! agent used when it served the portal, and it is why the fallback is a
//! `ServeFile` rather than a `not_found_service` — the latter forces the
//! fallback's status to 404, which would load the app *and* report it missing.

use std::path::{Path, PathBuf};

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

/// The portal's files, served at the root.
///
/// `directory` is where the built portal lives — `apps/portal/dist` in a
/// checkout. A directory with no `index.html` is a **build that has not been
/// run**, and it is reported as such rather than as a working site that happens
/// to be blank.
pub fn router(directory: impl AsRef<Path>) -> Router {
    let directory = directory.as_ref().to_path_buf();
    let index = directory.join("index.html");

    if !index.is_file() {
        return missing_build(&directory);
    }

    Router::new().fallback_service(ServeDir::new(&directory).fallback(ServeFile::new(&index)))
}

/// No portal at all: what `--no-portal` serves at every page path.
///
/// For development, where the portal runs on its own dev server and a build
/// left in its folder would otherwise be served here — an old copy that looks
/// exactly like the real one and silently lacks every change since.
pub fn api_only() -> Router {
    const PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">         <title>flexdeck API</title></head>         <body style=\"font-family: system-ui, sans-serif; max-width: 42rem;          margin: 4rem auto; line-height: 1.5\">         <h1>This is the flexdeck API</h1>         <p>It was started without the portal. In development the portal runs on          its own: <code>pnpm dev:web</code> serves it at          <a href=\"http://127.0.0.1:5173\">127.0.0.1:5173</a>.</p>         </body></html>";
    Router::new().fallback(|| async { axum::response::Html(PAGE) })
}

/// A page saying the portal has not been built.
///
/// A bare 404 at `/` reads as a bug in the server rather than a missing build
/// step, and the fix is different in each case. So this says which one it is, and
/// what to run.
fn missing_build(directory: &Path) -> Router {
    let page = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>flexdeck</title></head>\
         <body style=\"font-family: system-ui, sans-serif; max-width: 42rem; \
         margin: 4rem auto; line-height: 1.5\">\
         <h1>The portal has not been built</h1>\
         <p>This server is running and its API is available, but no portal was found at:</p>\
         <pre>{}</pre>\
         <p>Build it with <code>pnpm --filter @flexdeck/portal build</code>, then restart.</p>\
         </body></html>",
        escape_html(&directory.display().to_string())
    );

    Router::new().fallback(move || {
        let page = page.clone();
        async move { axum::response::Html(page) }
    })
}

/// Escape the characters that matter inside an HTML text node.
///
/// The path is operator-supplied rather than remote, but interpolating a path
/// into HTML without escaping is not a habit worth forming.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Where the built portal is, relative to a checkout.
///
/// Resolved from the crate's own location rather than the working directory, so
/// running the server from anywhere finds the same files. A path that depends on
/// where you happened to be standing is a bug that only shows up on someone
/// else's machine.
pub fn default_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/portal/dist")
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("apps/portal/dist"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn built() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temp dir");
        std::fs::write(directory.path().join("index.html"), "<h1>the portal</h1>").expect("writes");
        std::fs::write(directory.path().join("app.js"), "console.log(1)").expect("writes");
        directory
    }

    async fn get(router: Router, path: &str) -> (StatusCode, String) {
        let response = router
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("a request"),
            )
            .await
            .expect("responds");

        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");

        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    #[tokio::test]
    async fn the_portal_is_served_at_the_root() {
        let directory = built();
        let (status, body) = get(router(directory.path()), "/").await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("the portal"));
    }

    #[tokio::test]
    async fn a_deep_link_reaches_the_app_rather_than_a_404() {
        // The app owns its own routing, so the server must not decide that
        // `/desktops/abc` does not exist.
        let directory = built();
        let (status, body) = get(router(directory.path()), "/desktops/abc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("the portal"));
    }

    #[tokio::test]
    async fn a_real_file_is_served_as_itself() {
        // The fallback must not swallow assets, or the app would load and then
        // find its own script replaced by HTML.
        let directory = built();
        let (status, body) = get(router(directory.path()), "/app.js").await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("console.log"));
    }

    #[tokio::test]
    async fn a_missing_build_says_so_rather_than_serving_a_blank_page() {
        let directory = tempfile::tempdir().expect("temp dir");
        let (status, body) = get(router(directory.path()), "/").await;

        assert_eq!(status, StatusCode::OK);
        assert!(
            body.contains("has not been built"),
            "a blank page reads as a server bug, not a missing build step"
        );
        assert!(body.contains("pnpm"), "and it must say how to fix it");
    }

    #[test]
    fn a_path_is_escaped_before_going_into_the_page() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }
}
