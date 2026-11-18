//! Just enough iCalendar (RFC 5545): read `VEVENT`s, write one.

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::Serialize;

/// One event as shown to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// The event's `UID` (what `calendar_delete_event` takes).
    pub uid: String,
    /// Title.
    pub summary: String,
    /// Start: `YYYY-MM-DD` for an all-day event, else `YYYY-MM-DDTHH:MM:SS` (+`Z` for UTC).
    pub start: String,
    /// End, same format; empty when the event has none.
    pub end: String,
    /// Place, if any.
    pub location: String,
    /// Free text, if any (untrusted: written by whoever made the event).
    pub description: String,
}

fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        lines.push(raw.to_owned());
    }
    lines
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut it = v.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(o) => out.push(o),
            None => {}
        }
    }
    out
}

/// `20261120T090000Z` / `20261120T090000` / `20261120` to a readable form.
fn date_value(v: &str) -> String {
    let v = v.trim();
    if let Ok(d) = NaiveDate::parse_from_str(v, "%Y%m%d") {
        return d.format("%Y-%m-%d").to_string();
    }
    let (body, z) = v.strip_suffix('Z').map_or((v, ""), |b| (b, "Z"));
    NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S").map_or_else(
        |_| v.to_owned(),
        |dt| format!("{}{z}", dt.format("%Y-%m-%dT%H:%M:%S")),
    )
}

/// All `VEVENT`s of one iCalendar document. Sub-components (alarms) are skipped.
#[must_use]
pub fn parse_events(text: &str) -> Vec<Event> {
    let mut out = Vec::new();
    let mut cur: Option<Event> = None;
    let mut depth = 0usize;
    for line in unfold(text) {
        let upper = line.to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            cur = Some(Event {
                uid: String::new(),
                summary: String::new(),
                start: String::new(),
                end: String::new(),
                location: String::new(),
                description: String::new(),
            });
            depth = 0;
            continue;
        }
        if upper == "END:VEVENT" {
            out.extend(cur.take());
            continue;
        }
        let Some(ev) = cur.as_mut() else { continue };
        if upper.starts_with("BEGIN:") {
            depth += 1;
            continue;
        }
        if upper.starts_with("END:") {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth > 0 {
            continue;
        }
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let name = head.split(';').next().unwrap_or("").to_ascii_uppercase();
        match name.as_str() {
            "UID" => value.trim().clone_into(&mut ev.uid),
            "SUMMARY" => ev.summary = unescape(value),
            "LOCATION" => ev.location = unescape(value),
            "DESCRIPTION" => ev.description = unescape(value),
            "DTSTART" => ev.start = date_value(value),
            "DTEND" => ev.end = date_value(value),
            _ => {}
        }
    }
    out
}

fn escape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// A moment the agent gave: RFC 3339 or a plain date (all-day).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// `YYYY-MM-DD`.
    Day(NaiveDate),
    /// An instant.
    At(DateTime<Utc>),
}

impl When {
    /// Parse `YYYY-MM-DD` or an RFC 3339 timestamp (any offset).
    ///
    /// # Errors
    /// A message naming the accepted formats.
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return Ok(Self::Day(d));
        }
        DateTime::parse_from_rfc3339(s)
            .map(|d| Self::At(d.with_timezone(&Utc)))
            .map_err(|_| {
                format!(
                    "`{s}` is not a date: use YYYY-MM-DD or RFC 3339 (2026-11-20T09:00:00+01:00)"
                )
            })
    }

    fn ical(self) -> (&'static str, String) {
        match self {
            Self::Day(d) => (";VALUE=DATE", d.format("%Y%m%d").to_string()),
            Self::At(t) => ("", t.format("%Y%m%dT%H%M%SZ").to_string()),
        }
    }

    /// Instant used for `CalDAV` time ranges (a day starts at 00:00 UTC).
    #[must_use]
    pub fn utc(self) -> DateTime<Utc> {
        match self {
            Self::Day(d) => d.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc(),
            Self::At(t) => t,
        }
    }
}

/// Build the `.ics` body of a new event.
#[must_use]
pub fn build_event(
    uid: &str,
    summary: &str,
    start: When,
    end: When,
    location: Option<&str>,
    description: Option<&str>,
) -> String {
    let (sp, sv) = start.ical();
    let (ep, ev) = end.ical();
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//Praxi//Calendar//EN".to_owned(),
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", Utc::now().format("%Y%m%dT%H%M%SZ")),
        format!("DTSTART{sp}:{sv}"),
        format!("DTEND{ep}:{ev}"),
        format!("SUMMARY:{}", escape(summary)),
    ];
    if let Some(l) = location.filter(|l| !l.is_empty()) {
        lines.push(format!("LOCATION:{}", escape(l)));
    }
    if let Some(d) = description.filter(|d| !d.is_empty()) {
        lines.push(format!("DESCRIPTION:{}", escape(d)));
    }
    lines.push("END:VEVENT".to_owned());
    lines.push("END:VCALENDAR".to_owned());
    lines.join(
        "
",
    ) + "
"
}
