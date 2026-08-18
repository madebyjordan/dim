use displaydoc::Display;
use std::sync::Arc;
use thiserror::Error;

#[derive(Clone, Debug, Display, Error)]
pub enum DatabaseError {
    /// Generic database error: {0:?}
    DatabaseError(Arc<sqlx::error::Error>),
}

impl From<sqlx::error::Error> for DatabaseError {
    fn from(e: sqlx::error::Error) -> DatabaseError {
        Self::DatabaseError(e.into())
    }
}

impl DatabaseError {
    pub fn is_unique_violation(&self) -> bool {
        match self {
            Self::DatabaseError(error) => error
                .as_database_error()
                .and_then(|error| error.code())
                // SQLite's extended result codes for UNIQUE and PRIMARY KEY constraints.
                .is_some_and(|code| matches!(code.as_ref(), "2067" | "1555")),
        }
    }
}
