//! Domain error type for failures the application branches on.
//! Top-level orchestration uses `anyhow::Result`; these are the typed kinds.

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("failed to read config `{path}`: {reason}")]
    Config { path: String, reason: String },

    #[error("failed to read ledger `{path}`: {reason}")]
    Ledger { path: String, reason: String },

    #[error("portfolio error: {0}")]
    Portfolio(#[from] coinbasis::PortfolioError),

    #[error("price source error: {0}")]
    Price(String),

    #[error("cache error: {0}")]
    Cache(String),

    #[error("export error: {0}")]
    Export(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_displays_path() {
        let e = AppError::Config {
            path: "config.json".into(),
            reason: "missing field `ledger_path`".into(),
        };
        let msg = e.to_string();
        assert!(msg.contains("config.json"));
        assert!(msg.contains("ledger_path"));
    }
}
