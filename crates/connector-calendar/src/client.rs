//! `CalDAV` client: REPORT (query), PUT (create) and DELETE over one calendar
//! collection. TLS is required (plain HTTP only for loopback), redirects are
//! never followed (the password must not travel to another host), and an
//! `href` the server returns is used only when it stays on the configured origin.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use quick_xml::Reader;
use quick_xml::events::Event as Xml;
use reqwest::{Method, StatusCode};
use secret_store::{SecretRef, SecretStore, resolve};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::ical::{self, Event, When};

/// Largest response body read from a server.
const MAX_BODY: usize = 4 << 20;

/// One calendar the agent can use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarAccount {
    /// Short name the agent uses to pick the calendar (e.g. `"personal"`).
    pub name: String,
    /// URL of the calendar collection (not the server root).
    pub url: String,
    /// Login name.
    pub username: String,
    /// Secret-store reference of the (app) password.
    pub password: SecretRef,
    /// Per-request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    20
}

/// Why a calendar call failed. Never contains the password.
#[derive(Debug, thiserror::Error)]
pub enum CalendarError {
    /// Bad configuration or arguments.
    #[error("{0}")]
    Invalid(String),
    /// The secret could not be read.
    #[error("cannot read the calendar password from the secret store: {0}")]
    Credential(String),
    /// Network or HTTP failure.
    #[error("calendar server: {0}")]
    Server(String),
}

/// A configured account with its resolved base URL.
pub struct CalendarClient {
    account: CalendarAccount,
    base: Url,
    store: Arc<dyn SecretStore>,
    http: reqwest::Client,
}

fn is_loopback(host: &str) -> bool {
    host == "localhost" || host == "::1" || host == "[::1]" || host.starts_with("127.")
}

impl CalendarClient {
    /// # Errors
    /// [`CalendarError::Invalid`] for a bad URL or plain HTTP to a non-loopback host.
    pub fn new(
        account: CalendarAccount,
        store: Arc<dyn SecretStore>,
    ) -> Result<Self, CalendarError> {
        let mut base = Url::parse(&account.url).map_err(|e| {
            CalendarError::Invalid(format!("calendar `{}`: bad url: {e}", account.name))
        })?;
        let host = base.host_str().unwrap_or_default().to_owned();
        match base.scheme() {
            "https" => {}
            "http" if is_loopback(&host) => {}
            _ => {
                return Err(CalendarError::Invalid(format!(
                    "calendar `{}`: the url must be https (plain http only for localhost)",
                    account.name
                )));
            }
        }
        if !base.username().is_empty() || base.password().is_some() {
            return Err(CalendarError::Invalid(format!(
                "calendar `{}`: put the login in `username`/`password`, not in the url",
                account.name
            )));
        }
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(account.timeout_secs.clamp(1, 120)))
            .build()
            .map_err(|e| CalendarError::Server(e.to_string()))?;
        Ok(Self {
            account,
            base,
            store,
            http,
        })
    }

    /// The account name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.account.name
    }

    async fn password(&self) -> Result<String, CalendarError> {
        let (store, r) = (Arc::clone(&self.store), self.account.password.clone());
        tokio::task::spawn_blocking(move || {
            resolve(store.as_ref(), &r).map(|v| v.expose().to_string())
        })
        .await
        .map_err(|e| CalendarError::Credential(e.to_string()))?
        .map_err(|e| CalendarError::Credential(e.to_string()))
    }

    async fn send(
        &self,
        method: &str,
        url: Url,
        headers: &[(&str, &str)],
        body: Option<String>,
    ) -> Result<(StatusCode, String), CalendarError> {
        let pw = self.password().await?;
        let method = Method::from_bytes(method.as_bytes())
            .map_err(|e| CalendarError::Server(e.to_string()))?;
        let mut req = self
            .http
            .request(method, url)
            .basic_auth(&self.account.username, Some(pw));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        if let Some(b) = body {
            req = req.body(b);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| CalendarError::Server(redact(&e.to_string())))?;
        let status = resp.status();
        if status.is_redirection() {
            return Err(CalendarError::Server(format!(
                "the server answered with a redirect ({status}); use the final calendar url"
            )));
        }
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| CalendarError::Server(redact(&e.to_string())))?;
        if bytes.len() > MAX_BODY {
            return Err(CalendarError::Server("response too large".into()));
        }
        Ok((status, String::from_utf8_lossy(&bytes).into_owned()))
    }

    fn check(status: StatusCode, what: &str) -> Result<(), CalendarError> {
        if status.is_success() {
            return Ok(());
        }
        Err(CalendarError::Server(match status.as_u16() {
            401 | 403 => format!("{what}: the server refused the login or the action ({status})"),
            404 => format!("{what}: not found ({status}); check the calendar url"),
            _ => format!("{what}: HTTP {status}"),
        }))
    }

    async fn report(&self, body: String) -> Result<(StatusCode, String), CalendarError> {
        self.send(
            "REPORT",
            self.base.clone(),
            &[
                ("Depth", "1"),
                ("Content-Type", "application/xml; charset=utf-8"),
            ],
            Some(body),
        )
        .await
    }

    /// Events overlapping `[from, to)`, recurring ones expanded by the server.
    ///
    /// # Errors
    /// See [`CalendarError`].
    pub async fn list(
        &self,
        from: When,
        to: When,
        limit: usize,
    ) -> Result<Vec<Event>, CalendarError> {
        let (a, b) = (stamp(from.utc()), stamp(to.utc()));
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<c:calendar-query xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
<d:prop><c:calendar-data><c:expand start=\"{a}\" end=\"{b}\"/></c:calendar-data></d:prop>\
<c:filter><c:comp-filter name=\"VCALENDAR\"><c:comp-filter name=\"VEVENT\">\
<c:time-range start=\"{a}\" end=\"{b}\"/></c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"
        );
        let (status, text) = self.report(body).await?;
        Self::check(status, "listing events")?;
        let mut events: Vec<Event> = multistatus(&text)
            .into_iter()
            .flat_map(|r| ical::parse_events(&r.data))
            .collect();
        events.sort_by(|x, y| x.start.cmp(&y.start));
        events.truncate(limit);
        Ok(events)
    }

    /// Create an event; returns its new `UID`. Invites nobody (no attendees are written).
    ///
    /// # Errors
    /// See [`CalendarError`].
    pub async fn create(
        &self,
        summary: &str,
        start: When,
        end: When,
        location: Option<&str>,
        description: Option<&str>,
    ) -> Result<String, CalendarError> {
        if end.utc() <= start.utc() {
            return Err(CalendarError::Invalid("`end` must be after `start`".into()));
        }
        let uid = format!("{}@praxi", uuid::Uuid::now_v7());
        let ics = ical::build_event(&uid, summary, start, end, location, description);
        let url = self
            .base
            .join(&format!("{uid}.ics"))
            .map_err(|e| CalendarError::Invalid(e.to_string()))?;
        let (status, _) = self
            .send(
                "PUT",
                url,
                &[
                    ("Content-Type", "text/calendar; charset=utf-8"),
                    ("If-None-Match", "*"),
                ],
                Some(ics),
            )
            .await?;
        Self::check(status, "creating the event")?;
        Ok(uid)
    }

    /// Delete the event with this `UID`. Returns `false` when there is none.
    ///
    /// # Errors
    /// See [`CalendarError`].
    pub async fn delete(&self, uid: &str) -> Result<bool, CalendarError> {
        let want = quick_xml::escape::escape(uid);
        let body = format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<c:calendar-query xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">\
<d:prop><d:getetag/></d:prop>\
<c:filter><c:comp-filter name=\"VCALENDAR\"><c:comp-filter name=\"VEVENT\">\
<c:prop-filter name=\"UID\"><c:text-match collation=\"i;octet\">{want}</c:text-match></c:prop-filter>\
</c:comp-filter></c:comp-filter></c:filter></c:calendar-query>"
        );
        let (status, text) = self.report(body).await?;
        Self::check(status, "looking up the event")?;
        let Some(found) = multistatus(&text).into_iter().find(|r| !r.href.is_empty()) else {
            return Ok(false);
        };
        let url = self
            .base
            .join(&found.href)
            .map_err(|e| CalendarError::Server(format!("bad href from the server: {e}")))?;
        if url.origin() != self.base.origin() || !url.path().starts_with(self.base.path()) {
            return Err(CalendarError::Server(
                "the server pointed outside the configured calendar; refusing".into(),
            ));
        }
        let (status, _) = self.send("DELETE", url, &[], None).await?;
        if status == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        Self::check(status, "deleting the event")?;
        Ok(true)
    }
}

fn stamp(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Remove anything that looks like a credential-bearing URL from a transport error text.
fn redact(s: &str) -> String {
    s.split_whitespace()
        .map(|w| {
            if w.contains("://") && w.contains('@') {
                "<url>"
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Default)]
struct Response {
    href: String,
    data: String,
}

#[derive(PartialEq, Clone, Copy)]
enum Field {
    None,
    Href,
    Data,
}

/// The `response` entries of a multistatus document.
fn multistatus(xml: &str) -> Vec<Response> {
    let mut reader = Reader::from_str(xml);
    let mut out: Vec<Response> = Vec::new();
    let mut field = Field::None;
    loop {
        let piece: Option<String> = match reader.read_event() {
            Ok(Xml::Start(e)) => {
                field = match e.local_name().as_ref() {
                    "response" => {
                        out.push(Response::default());
                        Field::None
                    }
                    "href" => Field::Href,
                    "calendar-data" => Field::Data,
                    _ => Field::None,
                };
                None
            }
            Ok(Xml::End(_)) => {
                field = Field::None;
                None
            }
            Ok(Xml::Text(t)) => Some(t.xml10_content().into_owned()),
            Ok(Xml::CData(t)) => Some(t.xml10_content().into_owned()),
            Ok(Xml::GeneralRef(r)) => match r.resolve_char_ref() {
                Ok(Some(c)) => Some(c.to_string()),
                _ => match &*r {
                    "lt" => Some("<".to_owned()),
                    "gt" => Some(">".to_owned()),
                    "amp" => Some("&".to_owned()),
                    "quot" => Some("\"".to_owned()),
                    "apos" => Some("'".to_owned()),
                    _ => None,
                },
            },
            Ok(Xml::Eof) | Err(_) => break,
            _ => None,
        };
        if let (Some(p), Some(last)) = (piece, out.last_mut()) {
            match field {
                Field::Href => last.href.push_str(&p),
                Field::Data => last.data.push_str(&p),
                Field::None => {}
            }
        }
    }
    for r in &mut out {
        r.href = r.href.trim().to_owned();
    }
    out
}
