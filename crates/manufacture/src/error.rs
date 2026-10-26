//! The crate's error type. Every failure is a value: nothing here panics on hostile input.

/// What can go wrong while describing, planning or posting a job.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ManufactureError {
    /// A parameter, tool, setup or operation is not usable as given.
    #[error("{0}")]
    Invalid(String),
    /// Valid, but outside what the planner does yet.
    #[error("not supported yet: {0}")]
    Unsupported(String),
    /// The request would take more work or memory than the planner allows.
    #[error("too large: {0}")]
    Limit(String),
}

pub type Result<T> = std::result::Result<T, ManufactureError>;

/// `Err(Invalid)` with a message.
pub(crate) fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(ManufactureError::Invalid(msg.into()))
}

/// `Ok(x)` when `x` is finite, else an error naming `what`.
pub(crate) fn finite(x: f64, what: &str) -> Result<f64> {
    if x.is_finite() { Ok(x) } else { invalid(format!("{what} must be a finite number")) }
}
