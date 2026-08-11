use axum::response::IntoResponse;
use axum::response::Response;
use axum::Json;
use dim_core::errors::DimError;
use dim_core::stream_tracking::TrackingError;
use dim_database::DatabaseError;
use http::header::HeaderName;
use http::StatusCode;
use serde::Serialize;
use uuid::Uuid;

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Debug, Serialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
    request_id: String,
}

#[derive(Debug, Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: &'static str,
}

pub fn api_error(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    let request_id = Uuid::new_v4().to_string();
    let mut response = (
        status,
        Json(ApiErrorEnvelope {
            error: ApiErrorBody { code, message },
            request_id: request_id.clone(),
        }),
    )
        .into_response();
    response.headers_mut().insert(
        REQUEST_ID_HEADER,
        request_id.parse().expect("UUID is a valid header value"),
    );
    response
}

/// Wrapper for DimError that implements IntoResponse for Axum compatibility
#[derive(Debug)]
pub struct DimErrorWrapper(pub DimError);

impl From<DimError> for DimErrorWrapper {
    fn from(error: DimError) -> Self {
        Self(error)
    }
}

impl From<DatabaseError> for DimErrorWrapper {
    fn from(error: DatabaseError) -> Self {
        Self(DimError::DatabaseError {
            description: error.to_string(),
        })
    }
}

impl From<sqlx::Error> for DimErrorWrapper {
    fn from(error: sqlx::Error) -> Self {
        Self(DimError::DatabaseError {
            description: error.to_string(),
        })
    }
}

impl From<dim_core::errors::StreamingErrors> for DimErrorWrapper {
    fn from(error: dim_core::errors::StreamingErrors) -> Self {
        Self(DimError::StreamingError(error))
    }
}

impl From<nightfall::error::NightfallError> for DimErrorWrapper {
    fn from(error: nightfall::error::NightfallError) -> Self {
        Self(DimError::StreamingError(
            dim_core::errors::StreamingErrors::OtherNightfall(error),
        ))
    }
}

impl From<TrackingError> for DimErrorWrapper {
    fn from(error: TrackingError) -> Self {
        match error {
            TrackingError::NotOwner => Self(DimError::Unauthorized),
            TrackingError::NotFound => Self(DimError::StreamingError(
                dim_core::errors::StreamingErrors::SessionDoesntExist,
            )),
            TrackingError::AdmissionLimited { .. } => Self(DimError::StreamingError(
                dim_core::errors::StreamingErrors::AdmissionLimited(error.to_string()),
            )),
            TrackingError::InvalidMetadata => Self(DimError::StreamingError(
                dim_core::errors::StreamingErrors::InvalidMetadata(error.to_string()),
            )),
            TrackingError::InvalidSelection => Self(DimError::StreamingError(
                dim_core::errors::StreamingErrors::InvalidRequest,
            )),
            TrackingError::Transcoder(_) => Self(DimError::StreamingError(
                dim_core::errors::StreamingErrors::ProcFailed,
            )),
        }
    }
}

impl IntoResponse for DimErrorWrapper {
    fn into_response(self) -> Response {
        let (status_code, code, message) = match &self.0 {
            DimError::NotFoundError => (
                StatusCode::NOT_FOUND,
                "not_found",
                "The requested resource was not found.",
            ),
            DimError::Unauthenticated | DimError::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                "session_expired",
                "Sign in to continue.",
            ),
            DimError::Unauthorized => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "You do not have permission to perform this action.",
            ),
            DimError::InvalidMediaType => (
                StatusCode::BAD_REQUEST,
                "invalid_media_type",
                "Choose a supported media type.",
            ),
            DimError::MissingFieldInBody { .. } => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The request is missing a required value.",
            ),
            DimError::UnsupportedFile => (
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "unsupported_file",
                "This file type is not supported.",
            ),
            DimError::LibraryNotFound => (
                StatusCode::NOT_FOUND,
                "library_not_found",
                "The requested library was not found.",
            ),
            DimError::NoToken => (
                StatusCode::BAD_REQUEST,
                "invite_required",
                "An invite token is required.",
            ),
            DimError::UsernameNotAvailable => (
                StatusCode::CONFLICT,
                "username_unavailable",
                "That username is not available.",
            ),
            DimError::UploadFailed => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "upload_failed",
                "The upload could not be completed.",
            ),
            DimError::DatabaseError { .. } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Dim could not complete the request.",
            ),
            DimError::StreamingError(dim_core::errors::StreamingErrors::SessionDoesntExist) => (
                StatusCode::NOT_FOUND,
                "playback_session_not_found",
                "The playback session no longer exists.",
            ),
            DimError::StreamingError(dim_core::errors::StreamingErrors::InvalidRequest) => (
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The playback request is invalid.",
            ),
            DimError::StreamingError(dim_core::errors::StreamingErrors::AdmissionLimited(_)) => (
                StatusCode::TOO_MANY_REQUESTS,
                "playback_capacity_exhausted",
                "Playback capacity is currently exhausted. Try again shortly.",
            ),
            DimError::StreamingError(dim_core::errors::StreamingErrors::InvalidMetadata(_)) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_media_metadata",
                "This media cannot currently be played.",
            ),
            DimError::StreamingError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "playback_failed",
                "Playback could not be started.",
            ),
            DimError::ScannerError(_) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "scan_failed",
                "The library scan failed.",
            ),
            DimError::CookieError(_) => (
                StatusCode::BAD_REQUEST,
                "invalid_session",
                "The session token is invalid.",
            ),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "Dim could not complete the request.",
            ),
        };
        if status_code.is_server_error() {
            tracing::error!(error = ?self.0, "API request failed");
        }
        api_error(status_code, code, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::body::to_bytes;

    #[tokio::test]
    async fn internal_errors_are_safe_and_correlated() {
        let response = DimErrorWrapper(DimError::DatabaseError {
            description: "SQLITE secret path /private/db".into(),
        })
        .into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let header = response.headers()[REQUEST_ID_HEADER]
            .to_str()
            .unwrap()
            .to_owned();
        let body = to_bytes(response.into_body()).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["request_id"], header);
        assert_eq!(value["error"]["code"], "internal_error");
        assert!(!String::from_utf8_lossy(&body).contains("SQLITE"));
    }
}
