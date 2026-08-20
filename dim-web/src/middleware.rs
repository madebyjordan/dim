use axum::extract::FromRequestParts;
use axum::extract::State;
use axum::http::request::Parts;
use dim_core::errors::DimError;
use dim_database::user::Session;
use dim_database::user::User;
use dim_database::DbConnection;

use crate::error::REQUEST_ID_HEADER;
use crate::DimErrorWrapper;
use dim_core::settings::SettingsStore;
use dim_core::stream_tracking::{RemoteHlsStage, RemotePlaybackState, RemoteRequestAttribution};
use uuid::Uuid;

fn classify_hls_request_origin(
    playback_state: Option<RemotePlaybackState>,
    peer_ip: Option<std::net::IpAddr>,
    host_ip: Option<std::net::IpAddr>,
    user_agent: &str,
) -> RemoteRequestAttribution {
    match playback_state {
        Some(RemotePlaybackState::Prepared | RemotePlaybackState::HandoffRequested) | None => {
            RemoteRequestAttribution::SenderPreflight
        }
        Some(
            RemotePlaybackState::HandoffStalled
            | RemotePlaybackState::Failed
            | RemotePlaybackState::Disconnected,
        ) => RemoteRequestAttribution::DisconnectedOrStale,
        Some(
            RemotePlaybackState::WirelessRouteReported
            | RemotePlaybackState::MediaDeliveryConfirmed,
        ) => match (peer_ip, host_ip) {
            (Some(peer), Some(host)) if peer != host => {
                RemoteRequestAttribution::RemoteNetworkCandidate
            }
            (Some(_), Some(_))
                if user_agent.contains("AppleCoreMedia") || user_agent.contains("AirPlay") =>
            {
                RemoteRequestAttribution::AppleMediaIntermediaryCandidate
            }
            (Some(_), Some(_)) => RemoteRequestAttribution::SenderOrLocalProxy,
            _ => RemoteRequestAttribution::OriginUnresolved,
        },
    }
}

pub async fn trace_remote_playback<B>(
    State(state): State<crate::AppState>,
    req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> axum::response::Response {
    let path = req.uri().path().to_owned();
    let peer = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|connect| connect.0);
    let user_agent = req
        .headers()
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_owned();
    let range = req
        .headers()
        .get(axum::http::header::RANGE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("none")
        .to_owned();
    let host_ip = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(':').next())
        .and_then(|value| value.parse::<std::net::IpAddr>().ok());
    let session_id = path
        .strip_prefix("/api/v1/remote/")
        .and_then(|rest| rest.split('/').next())
        .and_then(|value| Uuid::parse_str(value).ok());
    let remote_playback_state = match session_id {
        Some(gid) => state.stream_tracking.remote_playback_state(&gid).await,
        None => None,
    };
    let request_attribution = classify_hls_request_origin(
        remote_playback_state,
        peer.map(|value| value.ip()),
        host_ip,
        &user_agent,
    );
    let stage = if path.ends_with("/master.m3u8") {
        RemoteHlsStage::MasterPlaylist
    } else if path.ends_with("/index.m3u8") {
        RemoteHlsStage::MediaPlaylist
    } else if path.ends_with("/init.mp4") {
        RemoteHlsStage::InitFragment
    } else if path.ends_with(".m4s") {
        RemoteHlsStage::MediaSegment
    } else {
        RemoteHlsStage::Unknown
    };
    let segment_number = path
        .rsplit('/')
        .next()
        .and_then(|value| value.strip_suffix(".m4s"))
        .and_then(|value| value.parse::<u64>().ok());
    let started_at = std::time::Instant::now();

    tracing::info!(
        session_id = ?session_id,
        hls_stage = ?stage,
        segment_number,
        remote_path = %path,
        remote_playback_state = ?remote_playback_state,
        request_attribution = ?request_attribution,
        peer = ?peer,
        user_agent = %user_agent,
        range = %range,
        "HLS transport request started"
    );
    let response = next.run(req).await;
    let successful = response.status().is_success();
    tracing::info!(
        session_id = ?session_id,
        hls_stage = ?stage,
        segment_number,
        remote_path = %path,
        remote_playback_state = ?remote_playback_state,
        request_attribution = ?request_attribution,
        peer = ?peer,
        status = %response.status(),
        elapsed_ms = started_at.elapsed().as_millis(),
        "HLS transport request finished"
    );
    if let Some(gid) = session_id {
        state
            .stream_tracking
            .observe_remote_hls_response(&gid, stage, request_attribution, &path, successful)
            .await;
    }
    response
}

#[cfg(test)]
mod remote_playback_tests {
    use super::*;

    #[test]
    fn preflight_is_never_attributed_to_a_receiver() {
        let sender = "192.168.1.160".parse().unwrap();
        let receiver = "192.168.1.80".parse().unwrap();
        assert_eq!(
            classify_hls_request_origin(
                Some(RemotePlaybackState::Prepared),
                Some(receiver),
                Some(sender),
                "AppleCoreMedia"
            ),
            RemoteRequestAttribution::SenderPreflight
        );
    }

    #[test]
    fn wireless_sender_proxy_and_direct_receiver_evidence_are_distinct() {
        let sender = "192.168.1.160".parse().unwrap();
        let receiver = "192.168.1.80".parse().unwrap();
        assert_eq!(
            classify_hls_request_origin(
                Some(RemotePlaybackState::WirelessRouteReported),
                Some(sender),
                Some(sender),
                "Mozilla/5.0 Safari/605.1.15"
            ),
            RemoteRequestAttribution::SenderOrLocalProxy
        );
        assert_eq!(
            classify_hls_request_origin(
                Some(RemotePlaybackState::WirelessRouteReported),
                Some(receiver),
                Some(sender),
                "AppleCoreMedia/1.0"
            ),
            RemoteRequestAttribution::RemoteNetworkCandidate
        );
        assert_eq!(
            classify_hls_request_origin(
                Some(RemotePlaybackState::WirelessRouteReported),
                Some(sender),
                Some(sender),
                "AppleCoreMedia/1.0"
            ),
            RemoteRequestAttribution::AppleMediaIntermediaryCandidate
        );
    }
}

pub async fn request_id<B>(
    req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> axum::response::Response {
    let mut response = next.run(req).await;
    if response.status().is_client_error() || response.status().is_server_error() {
        let is_structured = response
            .headers()
            .get(axum::http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.starts_with("application/json"))
            .unwrap_or(false);
        if !is_structured {
            let status = response.status();
            let (code, message) = match status {
                axum::http::StatusCode::BAD_REQUEST => {
                    ("invalid_request", "The request is invalid.")
                }
                axum::http::StatusCode::UNPROCESSABLE_ENTITY => {
                    ("invalid_request", "The request is invalid.")
                }
                axum::http::StatusCode::UNAUTHORIZED => ("session_expired", "Sign in to continue."),
                axum::http::StatusCode::FORBIDDEN => (
                    "forbidden",
                    "You do not have permission to perform this action.",
                ),
                axum::http::StatusCode::NOT_FOUND => {
                    ("not_found", "The requested resource was not found.")
                }
                axum::http::StatusCode::CONFLICT => {
                    ("conflict", "The request conflicts with the current state.")
                }
                axum::http::StatusCode::SERVICE_UNAVAILABLE => {
                    ("server_unavailable", "Eclipse is temporarily unavailable.")
                }
                _ => ("internal_error", "Eclipse could not complete the request."),
            };
            if status.is_server_error() {
                tracing::error!(%status, "Legacy API error was mapped to the safe envelope");
            }
            response = crate::error::api_error(status, code, message);
        }
    }
    if !response.headers().contains_key(&REQUEST_ID_HEADER) {
        response.headers_mut().insert(
            REQUEST_ID_HEADER,
            uuid::Uuid::new_v4()
                .to_string()
                .parse()
                .expect("UUID is a valid header value"),
        );
    }
    response
}

pub async fn deployment_guard<B>(
    State(settings): State<SettingsStore>,
    req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> axum::response::Response {
    let headers = req.headers();
    let has_forwarded = headers.contains_key("forwarded")
        || headers.contains_key("x-forwarded-for")
        || headers.contains_key("x-forwarded-host")
        || headers.contains_key("x-forwarded-proto");
    if has_forwarded && !settings.running().trust_proxy_headers {
        return crate::error::api_error(
            axum::http::StatusCode::BAD_REQUEST,
            "untrusted_proxy_headers",
            "Forwarded headers are not accepted in this deployment mode.",
        );
    }
    if has_forwarded && settings.running().trust_proxy_headers {
        let trusted_peer = req
            .extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|connect| connect.0.ip().is_loopback())
            .unwrap_or(false);
        if !trusted_peer {
            return crate::error::api_error(
                axum::http::StatusCode::BAD_REQUEST,
                "untrusted_proxy",
                "Proxy headers are accepted only from a loopback proxy.",
            );
        }
    }

    let configured_bind = settings
        .running()
        .bind_address
        .parse::<std::net::IpAddr>()
        .ok();
    if !settings.running().https_reverse_proxy
        && configured_bind
            .map(|address| address.is_loopback())
            .unwrap_or(true)
    {
        if let Some(host) = headers
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok())
        {
            let authority = format!("http://{host}")
                .parse::<axum::http::Uri>()
                .ok()
                .and_then(|uri| uri.authority().cloned());
            let allowed = authority
                .as_ref()
                .map(|authority| {
                    authority.host().eq_ignore_ascii_case("localhost")
                        || authority
                            .host()
                            .parse::<std::net::IpAddr>()
                            .map(|address| address.is_loopback())
                            .unwrap_or(false)
                })
                .unwrap_or(false);
            if !allowed {
                return crate::error::api_error(
                    axum::http::StatusCode::BAD_REQUEST,
                    "host_not_allowed",
                    "The request host is not allowed for the configured listener.",
                );
            }
        }
    }

    if let Some(origin) = headers
        .get(axum::http::header::ORIGIN)
        .and_then(|value| value.to_str().ok())
    {
        let parsed = origin.parse::<axum::http::Uri>();
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|value| value.to_str().ok());
        let expected_scheme = if settings.running().https_reverse_proxy {
            "https"
        } else {
            "http"
        };
        let valid = parsed
            .ok()
            .map(|uri| {
                uri.scheme_str() == Some(expected_scheme)
                    && uri.authority().map(|authority| authority.as_str()) == host
            })
            .unwrap_or(false);
        if !valid {
            return crate::error::api_error(
                axum::http::StatusCode::FORBIDDEN,
                "origin_not_allowed",
                "The request origin is not allowed.",
            );
        }
    }
    next.run(req).await
}

/// Extractor for routes that require the authenticated user to have the owner role.
pub struct Owner;

#[axum::async_trait]
impl<S> FromRequestParts<S> for Owner
where
    S: Send + Sync,
{
    type Rejection = DimErrorWrapper;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user = parts
            .extensions
            .get::<User>()
            .ok_or(DimErrorWrapper(DimError::Unauthenticated))?;

        if user.has_role("owner") {
            Ok(Self)
        } else {
            Err(DimErrorWrapper(DimError::Unauthorized))
        }
    }
}

pub async fn verify_cookie_token<B>(
    State(conn): State<DbConnection>,
    mut req: axum::http::Request<B>,
    next: axum::middleware::Next<B>,
) -> Result<axum::response::Response, DimErrorWrapper> {
    let bearer = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let cookie = req
        .headers()
        .get(axum::http::header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookies| {
            cookies.split(';').find_map(|cookie| {
                let (name, value) = cookie.trim().split_once('=')?;
                (name == "dim_session").then_some(value)
            })
        });
    let token = bearer
        .or(cookie)
        .ok_or(DimErrorWrapper(DimError::Unauthenticated))?;

    if token.is_empty() {
        return Err(DimErrorWrapper(DimError::InvalidCredentials));
    }

    let mut tx = conn.read().begin().await.map_err(|_| {
        DimErrorWrapper(DimError::DatabaseError {
            description: String::from("Failed to start transaction"),
        })
    })?;
    let session = Session::verify(&mut tx, token)
        .await
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    let current_user = dim_database::user::User::get_by_id(&mut tx, session.user_id)
        .await
        .map_err(|_| DimErrorWrapper(DimError::InvalidCredentials))?;

    req.extensions_mut().insert(current_user);
    req.extensions_mut().insert(session);
    Ok(next.run(req).await)
}
