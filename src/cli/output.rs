use serde::Serialize;
use std::io::IsTerminal;

/// Determines how CLI output is formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Structured JSON with `{ ok, data, error }` envelope.
    Json,
    /// Human-readable text output.
    Pretty,
}

impl OutputMode {
    /// Auto-detect: JSON if stdout is not a TTY, Pretty if it is.
    /// Can be overridden by `--json` or `--plain` flags.
    pub fn detect(force_json: bool, force_plain: bool) -> Self {
        if force_json {
            OutputMode::Json
        } else if force_plain {
            OutputMode::Pretty
        } else if std::io::stdout().is_terminal() {
            OutputMode::Pretty
        } else {
            OutputMode::Json
        }
    }
}

/// Standard JSON output envelope for all commands.
#[derive(Debug, Serialize)]
pub struct JsonOutput<T: Serialize> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl<T: Serialize> JsonOutput<T> {
    pub fn success(data: T) -> Self {
        JsonOutput {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> JsonOutput<serde_json::Value> {
        JsonOutput {
            ok: false,
            data: None,
            error: Some(msg.into()),
        }
    }

    pub fn print(&self) {
        println!("{}", serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

/// Print a JSON success response.
pub fn print_json_ok<T: Serialize>(data: T) {
    JsonOutput::success(data).print();
}

/// Print a JSON error response.
pub fn print_json_err(msg: impl Into<String>) {
    JsonOutput::<serde_json::Value>::error(msg).print();
}
