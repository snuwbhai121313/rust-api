use axum::{
    extract::Query,
    http::{header, HeaderValue, Method, StatusCode, Uri},
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};

// ani_lib is the library exposed by the `ani-cli-rs` crate.
use ani_lib::{AnikotoCzClient, StreamLink, TranslationType};

// ---------------------------------------------------------------------------
// Request / response models
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamParams {
    q: String,
    #[serde(default = "default_ep")]
    ep: String,
    #[serde(default = "default_mode")]
    mode: String,
}

fn default_ep() -> String {
    "1".to_string()
}

fn default_mode() -> String {
    "sub".to_string()
}

#[derive(Serialize)]
struct StreamEntry {
    url: String,
    resolution: String,
    hls: bool,
    provider: String,
}

impl From<&StreamLink> for StreamEntry {
    fn from(link: &StreamLink) -> Self {
        StreamEntry {
            url: link.url.clone(),
            resolution: link.resolution.clone(),
            hls: link.hls,
            provider: link.provider.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Shared resolution logic
// ---------------------------------------------------------------------------

/// Resolve the first search hit for `query` and fetch its stream links.
/// Returns a descriptive error string on failure so handlers can map it
/// to the right status code.
async fn resolve_streams(query: &str, ep: &str, mode: &str) -> Result<Vec<StreamLink>, String> {
    // Parse the sub/dub mode; anything unrecognized falls back to sub.
    let translation = match mode.to_ascii_lowercase().as_str() {
        "dub" => TranslationType::Dub,
        _ => TranslationType::Sub,
    };

    // Build the Anikoto.cz client (anikoto2 catalog — the default provider).
    let client = AnikotoCzClient::new().map_err(|e| format!("client init failed: {e}"))?;

    // Search the catalog for the requested title.
    let results = client
        .search(query, translation)
        .await
        .map_err(|e| format!("search failed: {e}"))?;

    // Take the first hit — the API contract is "first match wins".
    let show = results
        .first()
        .ok_or_else(|| "Anime not found".to_string())?;

    // Resolve direct stream links for the requested episode.
    let streams = client
        .streams(&show.id, ep, translation)
        .await
        .map_err(|e| format!("stream resolution failed: {e}"))?;

    Ok(streams)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET / — service discovery payload.
async fn index() -> Json<serde_json::Value> {
    Json(json!({
        "name": "Anikoto Streaming API",
        "status": "online",
        "endpoints": {
            "stream": "/stream?q=naruto&ep=1&mode=sub",
            "redirect": "/redirect?q=naruto&ep=1&mode=sub"
        }
    }))
}

/// GET /stream?q=<title>&ep=<n>&mode=<sub|dub>
/// Returns JSON with every resolved stream link, or a JSON error.
async fn stream_handler(Query(params): Query<StreamParams>) -> Response {
    let query = params.q.trim().to_string();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Missing ?q=" })),
        )
            .into_response();
    }

    match resolve_streams(&query, &params.ep, &params.mode).await {
        Ok(streams) => {
            if streams.is_empty() {
                return (
                    StatusCode::NOT_FOUND,
                    Json(json!({ "error": "Anime not found" })),
                )
                    .into_response();
            }
            let entries: Vec<StreamEntry> = streams.iter().map(StreamEntry::from).collect();
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "query": query,
                    "episode": params.ep,
                    "mode": params.mode,
                    "streams": entries
                })),
            )
                .into_response()
        }
        Err(message) => {
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({ "success": false, "error": message }))).into_response()
        }
    }
}

/// GET /redirect?q=<title>&ep=<n>&mode=<sub|dub>
/// 302-redirects straight to the first stream URL.
async fn redirect_handler(Query(params): Query<StreamParams>) -> Response {
    let query = params.q.trim().to_string();
    if query.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Missing ?q=" })),
        )
            .into_response();
    }

    match resolve_streams(&query, &params.ep, &params.mode).await {
        Ok(streams) => match streams.first() {
            Some(link) => Redirect::temporary(&link.url).into_response(),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Anime not found" })),
            )
                .into_response(),
        },
        Err(message) => {
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(json!({ "success": false, "error": message }))).into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Server entrypoint
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Railway injects $PORT; default to 8080 locally.
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8080);

    // Permissive CORS so browser players can hit the API directly.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/stream", get(stream_handler))
        .route("/redirect", get(redirect_handler))
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind listener");

    println!("🍟♤ ｐ𝓞т𝐀tᵒ 🐟🎁 Anikoto API listening on 0.0.0.0:{port}");

    axum::serve(listener, app)
        .await
        .expect("server exited unexpectedly");
}
