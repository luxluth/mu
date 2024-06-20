mod config;
mod utils;

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header::CACHE_CONTROL, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use axum_extra::{headers::Range, TypedHeader};
use axum_range::{KnownSize, Ranged};
use config::Dir;
use mud::Media;
use std::io::Read;
use tokio::fs::File;
use tower::ServiceBuilder;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone)]
struct AppData {
    media: Media,
    dirs: Dir,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing::subscriber::set_global_default(FmtSubscriber::default())?;

    let mut host = "localhost".to_string();
    let mut port: u32 = 7700;

    let dirs = config::get_dirs();
    let m = utils::cache_resolve(&dirs.cache).await;
    let config_path = dirs.config.join("config.toml");
    let config = muconf::Config::get(&config_path);
    if let Some(network) = config.network {
        if let Some(p) = network.port {
            port = p;
        }

        if let Some(h) = network.host {
            host = h;
        }
    }

    let app = Router::new()
        .route("/", get(ping))
        .route("/media", get(media))
        .route("/audio/:id", get(audio))
        .route("/album/:id", get(album))
        .route("/cover/:handle", get(cover))
        .with_state(AppData { media: m, dirs })
        .layer(ServiceBuilder::new().layer(CorsLayer::permissive()));

    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    info!("MUD started on http://{host}:{port}");
    axum::serve(listener, app).await?;

    Ok(())
}

async fn cover(State(state): State<AppData>, Path(handle): Path<String>) -> Response {
    let path = state.dirs.cache.join("covers").join(handle);

    let mut buf = vec![];
    if let Ok(mut file) = std::fs::File::open(&path) {
        let _ = file.read_to_end(&mut buf);

        let body = Body::from(buf);
        let resp = Response::new(body);
        resp
    } else {
        warn!("Fail to retrieve the cover file `{}`", path.display());
        let buf = include_bytes!("./assets/default-cover.png");
        let body = Body::from(buf.as_slice());
        let mut resp = Response::new(body);

        resp.headers_mut().insert(
            CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=2419200, immutable"),
        );

        resp
    }
}

async fn album(State(state): State<AppData>, Path(id): Path<String>) -> Response {
    if let Some(album) = state.media.get_album(&id) {
        Json(album).into_response()
    } else {
        let mut response = format!("no album found with the id of {id}").into_response();
        *response.status_mut() = StatusCode::NOT_FOUND;
        response
    }
}

async fn audio(
    range: Option<TypedHeader<Range>>,
    State(state): State<AppData>,
    Path(id): Path<String>,
) -> Response {
    info!("{id}");
    if let Some(track) = state.media.get_song(&id) {
        let file = File::open(&track.file_path).await.unwrap();
        let body = KnownSize::file(file).await.unwrap();
        let r = range.clone().map(|TypedHeader(range)| range);
        let response = Ranged::new(r, body).try_respond();
        if let Ok(response) = response {
            return response.into_response();
        } else {
            let mut response =
                format!("An error occured while satisfying the request for {id}").into_response();
            *response.status_mut() = StatusCode::NOT_FOUND;
            response
        }
    } else {
        warn!("{id} not founded");
        let mut response = format!("no song found with the id of {id}").into_response();
        *response.status_mut() = StatusCode::NOT_FOUND;
        response
    }
}

async fn ping() -> String {
    format!("OK mud v{}", config::VERSION)
}

async fn media(State(state): State<AppData>) -> Json<Media> {
    Json(state.media)
}
