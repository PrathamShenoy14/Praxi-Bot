//! Calendar connector (`CalDAV`) as agent tools.
//!
//! - [`tools::LIST`] reads events in a time range (`Effect::Read`; the text is
//!   third-party data and is marked untrusted).
//! - [`tools::CREATE`] adds an event (`Effect::Write`; it never invites anyone).
//! - [`tools::DELETE`] removes one by `UID` (`Effect::Destructive`).
//! - Passwords are [`secret_store::SecretRef`]s resolved only when calling;
//!   TLS is required (plain HTTP only for loopback) and redirects are refused.

pub mod client;
pub mod ical;
pub mod tools;

use std::sync::Arc;

use secret_store::SecretStore;
use tool_registry::{Tool, ToolRegistry};

pub use client::{CalendarAccount, CalendarClient, CalendarError};
pub use ical::{Event, When};
pub use tools::TOOL_NAMES;

/// The configured calendars.
pub struct CalendarConnector {
    clients: Vec<CalendarClient>,
}

impl CalendarConnector {
    /// Build clients for `accounts`; an account that fails validation is
    /// skipped and reported in the second value.
    #[must_use]
    pub fn new(
        accounts: Vec<CalendarAccount>,
        store: &Arc<dyn SecretStore>,
    ) -> (Self, Vec<String>) {
        let mut clients = Vec::new();
        let mut problems = Vec::new();
        for a in accounts {
            if clients.iter().any(|c: &CalendarClient| c.name() == a.name) {
                problems.push(format!("calendar `{}` is listed twice", a.name));
                continue;
            }
            match CalendarClient::new(a, Arc::clone(store)) {
                Ok(c) => clients.push(c),
                Err(e) => problems.push(e.to_string()),
            }
        }
        (Self { clients }, problems)
    }

    /// Names of the usable calendars.
    #[must_use]
    pub fn names(&self) -> Vec<&str> {
        self.clients.iter().map(CalendarClient::name).collect()
    }

    /// The named calendar, or the only one when `name` is `None`.
    ///
    /// # Errors
    /// [`CalendarError::Invalid`] when the name is unknown or ambiguous.
    pub fn pick(&self, name: Option<&str>) -> Result<&CalendarClient, CalendarError> {
        match (name, self.clients.as_slice()) {
            (Some(n), cs) => cs.iter().find(|c| c.name() == n).ok_or_else(|| {
                CalendarError::Invalid(format!(
                    "no calendar named `{n}`; have: {}",
                    self.names().join(", ")
                ))
            }),
            (None, [only]) => Ok(only),
            (None, []) => Err(CalendarError::Invalid("no calendar is configured".into())),
            (None, _) => Err(CalendarError::Invalid(format!(
                "several calendars are configured; pass `account` ({})",
                self.names().join(", ")
            ))),
        }
    }
}

/// Register the calendar tools; returns how many were added.
#[must_use]
pub fn register(registry: &ToolRegistry, connector: &Arc<CalendarConnector>) -> usize {
    tools::tools(connector)
        .into_iter()
        .filter(|t: &Arc<dyn Tool>| registry.register(t.clone()))
        .count()
}

#[cfg(test)]
mod tests;
