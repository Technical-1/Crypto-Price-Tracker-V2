//! Domain error type. Expanded in Task 2.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("placeholder")]
    Placeholder,
}
