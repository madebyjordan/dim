use crate::AppState;
use axum::body;
use axum::body::Full;
use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::response::Response;

use crate::error::DimErrorWrapper;
use dim_core::fetcher::insert_into_queue;
use dim_database::asset;

use http::StatusCode;
use rust_embed::RustEmbed;

use std::path::PathBuf;

use serde::Deserialize;

cfg_if::cfg_if! {
    if #[cfg(feature = "embed_ui")] {

        #[derive(RustEmbed)]
        #[folder = "../eclipse/build/"]
        #[prefix = "/"]
        pub(self) struct Asset;
    } else {
        use rust_embed::Filenames;
        use std::borrow::Cow;

        pub(self) struct Asset;

        impl RustEmbed for Asset {
            fn get(_: &str) -> Option<Cow<'static, [u8]>> {
                None
            }

            fn iter() -> Filenames {
                unimplemented!()
            }
        }
    }
}

pub async fn frontend_route(uri: Uri) -> Result<impl IntoResponse, DimErrorWrapper> {
    let path = uri.path();
    if let Some(asset) = Asset::get(path) {
        return embedded_asset(path, asset.into_owned());
    }
    if let Some(index) = Asset::get("/index.html") {
        return Ok(Response::builder()
            .status(StatusCode::OK)
            .header("Content-Type", "text/html; charset=utf-8")
            .header("Cache-Control", "no-cache")
            .body(body::boxed(Full::from(index.into_owned())))
            .unwrap());
    }
    Err(dim_core::errors::DimError::NotFoundError.into())
}

#[derive(Deserialize)]
pub struct ImageParams {
    _w: Option<u32>,
    _h: Option<u32>,
    #[serde(default)]
    attach_accents: bool,
}

pub async fn get_image(
    State(AppState { conn, .. }): State<AppState>,
    Path(path): Path<String>,
    Query(params): Query<ImageParams>,
) -> Result<impl IntoResponse, DimErrorWrapper> {
    let meta_path = dim_core::core::METADATA_PATH
        .get()
        .ok_or(dim_core::errors::DimError::InternalServerError)?;
    let relative_path = dim_core::utils::safe_relative_path(path.as_str())
        .map_err(|_| dim_core::errors::DimError::NotFoundError)?;
    let file_path = dim_core::utils::safe_metadata_path(meta_path, &relative_path)
        .map_err(|_| dim_core::errors::DimError::NotFoundError)?;

    let mut url_path = PathBuf::from("images/");
    url_path.push(&relative_path);

    /*
    let image = if let (Some(w), Some(h)) = (resize_w, resize_h) {
        spawn_blocking(move ||  { resize_image(file_path, w, h).ok() }).await.unwrap()
    } else {
        tokio::fs::read(file_path).await.ok()
    };
    */

    let mut tx = conn.read().begin().await?;
    // FIXME (val): return not yet available error here as a hint that in the future this URL will
    // return 200 OK.
    if !file_path.exists() {
        if let Ok(x) = asset::Asset::get_url_by_file(&mut tx, &url_path).await {
            insert_into_queue(x, relative_path.to_string_lossy().into_owned(), true).await;
        }

        return Err(dim_core::errors::DimError::NotFoundError.into());
    }

    let image = tokio::fs::read(file_path).await.ok();

    let accents = match (image.as_ref(), params.attach_accents) {
        (Some(data), true) => {
            if let Ok(image) = image::load_from_memory(&data) {
                Some(
                    dominant_color::get_colors(image.as_bytes(), false)
                        .chunks_exact(3)
                        .map(|rgb| match rgb {
                            [r, g, b] => format!("#{r:02x}{g:02x}{b:02x}"),
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>()
                        .join(","),
                )
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(data) = image {
        let mut resp = Response::builder()
            .status(StatusCode::OK)
            .header("ContentType", "image/jpeg");

        if let Some(accents) = accents {
            resp = resp.header("X-IMAGE-ACCENTS", accents);
        }

        return Ok(resp.body(body::boxed(Full::from(data))).unwrap());
    }

    Err(dim_core::errors::DimError::NotFoundError.into())
}

pub async fn dist_static(uri: Uri) -> Result<impl IntoResponse, DimErrorWrapper> {
    let path = uri.path();
    match Asset::get(path) {
        Some(asset) => embedded_asset(path, asset.into_owned()),
        None => Err(dim_core::errors::DimError::NotFoundError.into()),
    }
}

fn embedded_asset(path: &str, data: Vec<u8>) -> Result<Response, DimErrorWrapper> {
    let path = PathBuf::from(path);
    let mime = match path.extension().and_then(|x| x.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("map") | Some("json") => "application/json",
        Some("css") => "text/css",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("data") => "application/octet-stream",
        _ => return Err(dim_core::errors::DimError::NotFoundError.into()),
    };
    let immutable = path
        .components()
        .any(|component| component.as_os_str() == "immutable");
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", mime)
        .header(
            "Cache-Control",
            if immutable {
                "public, max-age=31536000, immutable"
            } else {
                "no-cache"
            },
        )
        .body(body::boxed(Full::from(data)))
        .unwrap())
}

#[cfg(all(test, feature = "embed_ui"))]
mod frontend_tests {
    use super::*;

    #[tokio::test]
    async fn client_route_falls_back_to_the_eclipse_entrypoint() {
        let response = frontend_route("/play/42".parse().unwrap())
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Content-Type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(response.headers().get("Cache-Control").unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn immutable_svelte_assets_receive_long_lived_caching() {
        let asset = Asset::iter()
            .find(|name| name.contains("/_app/immutable/") && name.ends_with(".js"))
            .expect("the Eclipse build contains an immutable JavaScript asset");
        let response = frontend_route(asset.parse().unwrap())
            .await
            .unwrap()
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("Cache-Control").unwrap(),
            "public, max-age=31536000, immutable"
        );
    }
}
