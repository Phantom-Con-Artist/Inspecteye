//! Chooses between human-readable and JSON output for one command.
//!
//! JSON output is one object per invocation on stdout:
//!
//! ```json
//! { "schema_version": 1, "command": "status", "ok": true, "data": { ... } }
//! { "schema_version": 1, "command": "status", "ok": false,
//!   "error": { "kind": "not_initialized", "message": "..." } }
//! ```
//!
//! `data` is the command's result type serialized as-is, so the JSON shape
//! follows the core model rather than the terminal layout.

use serde::Serialize;

/// Bumped when a JSON shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

pub struct Output {
    json: bool,
    command: &'static str,
}

#[derive(Serialize)]
struct Envelope<'a, T: Serialize> {
    schema_version: u32,
    command: &'a str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<&'a T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ErrorBody<'a>>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    kind: &'a str,
    message: &'a str,
}

impl Output {
    pub fn new(json: bool, command: &'static str) -> Self {
        Self { json, command }
    }

    /// Prints the result: as JSON, or through the human renderer.
    pub fn emit<T: Serialize>(&self, data: &T, human: impl FnOnce(&T)) {
        if self.json {
            self.print(&Envelope {
                schema_version: SCHEMA_VERSION,
                command: self.command,
                ok: true,
                data: Some(data),
                error: None,
            });
        } else {
            human(data);
        }
    }

    /// A progress or context line shown to humans only.
    pub fn note(&self, text: &str) {
        if !self.json {
            println!("{text}");
        }
    }

    pub fn error(&self, kind: &str, message: &str) {
        if self.json {
            self.print(&Envelope::<()> {
                schema_version: SCHEMA_VERSION,
                command: self.command,
                ok: false,
                data: None,
                error: Some(ErrorBody { kind, message }),
            });
        } else {
            eprintln!("Error: {message}");
        }
    }

    fn print<T: Serialize>(&self, value: &T) {
        match serde_json::to_string_pretty(value) {
            Ok(text) => println!("{text}"),
            Err(e) => eprintln!("Error: could not serialize output: {e}"),
        }
    }
}
