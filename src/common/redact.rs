//! Helpers for hand-written `Debug` impls that must not print secrets.
//!
//! `#[instrument]` records handler arguments with their `Debug` output, so any
//! model that holds a password hash, private key, email or token must hide
//! those fields there (issue #36). `#[serde(skip_serializing)]` does not
//! affect `Debug`.

use std::fmt;

/// Placeholder printed instead of a secret value.
pub const REDACTED: &str = "[redacted]";

/// Prints `[redacted]` in place of a value.
pub struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Prints `Some([redacted])` or `None`, so logs still show whether the field
/// is set without showing its value.
pub struct RedactedOption<'a, T>(pub &'a Option<T>);

impl<T> fmt::Debug for RedactedOption<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(_) => f.debug_tuple("Some").field(&Redacted).finish(),
            None => f.write_str("None"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacted_option_hides_value_but_shows_presence() {
        let secret = Some("hunter2".to_string());
        assert_eq!(format!("{:?}", RedactedOption(&secret)), "Some([redacted])");
        assert_eq!(format!("{:?}", RedactedOption::<String>(&None)), "None");
    }
}
