use std::fmt::Write as _;

use axum::{
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// The dashboard assets, baked into the binary at compile time.
///
/// `folder` is resolved against `CARGO_MANIFEST_DIR` when the crate is built,
/// so the served bytes no longer depend on the process working directory — a
/// standalone binary started by systemd has no repo tree next to it.
#[derive(RustEmbed)]
#[folder = "src/interface/web/static/"]
struct Assets;

/// Quoted hex ETag for an asset's content hash.
///
/// Hand-rolled on purpose: `hex` is not a direct dependency (only a transitive
/// one), and 32 bytes do not justify adding a crate.
fn etag_for(hash: [u8; 32]) -> String {
    let mut etag = String::with_capacity(66);
    etag.push('"');
    for byte in hash {
        let _ = write!(etag, "{byte:02x}");
    }
    etag.push('"');
    etag
}

/// Serve an embedded asset, honouring conditional requests.
///
/// Handles both the `/static/…` route and the router fallback: rust-embed keys
/// files relative to `folder`, so the embedded names are `index.html`,
/// `app.js`, `style.css` — the request prefix is not part of the key and has to
/// be stripped.
pub async fn asset_handler(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    // "/static/app.js" and "/app.js" are both the embedded key "app.js".
    let path = path.strip_prefix("static/").unwrap_or(path);
    let path = if path.is_empty() { "index.html" } else { path };

    let Some(file) = Assets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let etag = etag_for(file.metadata.sha256_hash());
    if headers
        .get(header::IF_NONE_MATCH)
        .is_some_and(|value| value == etag.as_str())
    {
        return StatusCode::NOT_MODIFIED.into_response();
    }

    // The explicit content type is load-bearing: `IntoResponse for
    // Cow<'static, [u8]>` labels the body `application/octet-stream`, so
    // returning `file.data` alone would serve the dashboard as a download.
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype()),
            (header::ETAG, etag.as_str()),
        ],
        file.data,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::{asset_handler, etag_for, Assets};
    use axum::body::to_bytes;
    use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
    use std::borrow::Cow;

    async fn get(path: &str, headers: HeaderMap) -> axum::response::Response {
        asset_handler(path.parse::<Uri>().unwrap(), headers).await
    }

    fn header_of(response: &axum::response::Response, name: header::HeaderName) -> String {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn assets_are_embedded_not_read_from_disk() {
        // Cow::Borrowed(&'static [u8]) is only reachable when the bytes live in
        // the binary; the filesystem fallback would hand back Cow::Owned.
        for name in ["index.html", "app.js", "style.css"] {
            let file = Assets::get(name).expect("asset should be embedded");
            assert!(
                matches!(file.data, Cow::Borrowed(_)),
                "{name} was read from disk instead of the binary"
            );
        }
    }

    #[tokio::test]
    async fn root_serves_the_dashboard_html() {
        let response = get("/", HeaderMap::new()).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert!(header_of(&response, header::CONTENT_TYPE).starts_with("text/html"));

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert!(String::from_utf8_lossy(&body).contains("<title>NanoMon"));
    }

    #[tokio::test]
    async fn static_prefix_is_stripped_and_typed() {
        let js = get("/static/app.js", HeaderMap::new()).await;
        assert_eq!(js.status(), StatusCode::OK);
        // Guards the octet-stream default of IntoResponse for Cow<[u8]>.
        assert!(header_of(&js, header::CONTENT_TYPE).contains("javascript"));

        let css = get("/static/style.css", HeaderMap::new()).await;
        assert_eq!(css.status(), StatusCode::OK);
        assert!(header_of(&css, header::CONTENT_TYPE).starts_with("text/css"));
    }

    #[tokio::test]
    async fn assets_resolve_with_or_without_the_static_prefix() {
        assert_eq!(
            get("/app.js", HeaderMap::new()).await.status(),
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn unknown_paths_are_not_found() {
        assert_eq!(
            get("/nope.js", HeaderMap::new()).await.status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get("/static/nope.js", HeaderMap::new()).await.status(),
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn a_matching_etag_gets_an_empty_304() {
        let first = get("/static/app.js", HeaderMap::new()).await;
        let etag = header_of(&first, header::ETAG);
        assert!(etag.starts_with('"') && etag.ends_with('"'), "etag: {etag}");

        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_str(&etag).unwrap());
        let second = get("/static/app.js", headers).await;

        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        let body = to_bytes(second.into_body(), usize::MAX).await.unwrap();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn a_stale_etag_gets_the_asset_again() {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, HeaderValue::from_static("\"stale\""));

        assert_eq!(
            get("/static/app.js", headers).await.status(),
            StatusCode::OK
        );
    }

    #[test]
    fn etag_is_quoted_lowercase_hex() {
        let mut hash = [0u8; 32];
        hash[0] = 0x0a;
        hash[31] = 0xff;

        let etag = etag_for(hash);

        // 32 bytes -> 64 hex chars, plus the two quotes.
        assert_eq!(etag.len(), 66);
        assert!(etag.starts_with("\"0a"));
        assert!(etag.ends_with("ff\""));
        assert!(etag[1..65].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_assets_get_different_etags() {
        let js = etag_for(Assets::get("app.js").unwrap().metadata.sha256_hash());
        let css = etag_for(Assets::get("style.css").unwrap().metadata.sha256_hash());
        assert_ne!(js, css);
    }
}
