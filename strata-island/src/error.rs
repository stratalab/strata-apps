//! Demo error codes. Tests assert `code` / `class`, never display text.

use serde::Serialize;
use stratadb::EngineError;

#[derive(Clone, Debug, Serialize)]
pub struct IslandError {
    pub code: String,
}

impl IslandError {
    #[must_use]
    pub fn code(code: &'static str) -> Self {
        Self {
            code: code.to_owned(),
        }
    }

    #[must_use]
    pub fn engine(error: &EngineError) -> Self {
        Self {
            code: error.code().to_owned(),
        }
    }

    #[must_use]
    pub fn class(&self) -> &str {
        self.code.split('.').next().unwrap_or("invalid_argument")
    }
}

impl std::fmt::Display for IslandError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.code)
    }
}

impl std::error::Error for IslandError {}

impl From<EngineError> for IslandError {
    fn from(error: EngineError) -> Self {
        Self::engine(&error)
    }
}
