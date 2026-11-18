//! Offline tests against an in-process fake `CalDAV` server.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use secret_store::{MemoryStore, SecretRef, SecretStore};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tool_registry::{ContentPart, ToolContext, ToolError, ToolRegistry};

use crate::tools::tools;
use crate::{CalendarAccount, CalendarConnector, register};

type Events = Arc<Mutex<BTreeMap<String, String>>>;

struct Fake {
    port: u16,
    events: Events,
    seen_auth: Arc<Mutex<Vec<String>>>,
}

/// A tiny `CalDAV` server: `PUT /cal/<uid>.ics`, `DELETE`, and `REPORT` that
/// returns every stored event (or the one matching a UID filter). With
/// `evil_href` the REPORT answers point at another host.
async fn fake(evil_href: bool) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let events: Events = Arc::default();
    let seen_auth: Arc<Mutex<Vec<String>>> = Arc::default();
    let (ev, auth) = (events.clone(), seen_auth.clone());
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let (ev, auth) = (ev.clone(), auth.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 4096];
                let (head, body) = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let text = String::from_utf8_lossy(&buf).into_owned();
                    if let Some((h, b)) = text.split_once("\r\n\r\n") {
                        let len = h
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if b.len() >= len {
                            break (h.to_owned(), b.to_owned());
                        }
                    }
                };
                let mut lines = head.lines();
                let mut first = lines.next().unwrap_or("").split(' ');
                let (method, path) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
                for l in lines {
                    if l.to_ascii_lowercase().starts_with("authorization:") {
                        auth.lock().unwrap().push(l.to_owned());
                    }
                }
                let (status, payload) = match method {
                    "PUT" => {
                        let uid = path.rsplit('/').next().unwrap().trim_end_matches(".ics");
                        ev.lock().unwrap().insert(uid.to_owned(), body);
                        ("201 Created", String::new())
                    }
                    "DELETE" => {
                        let uid = path.rsplit('/').next().unwrap().trim_end_matches(".ics");
                        match ev.lock().unwrap().remove(uid) {
                            Some(_) => ("204 No Content", String::new()),
                            None => ("404 Not Found", String::new()),
                        }
                    }
                    "REPORT" => {
                        let mut xml = String::from(
                            "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:caldav\">",
                        );
                        for (uid, ics) in ev.lock().unwrap().iter() {
                            if body.contains("text-match") && !body.contains(uid.as_str()) {
                                continue;
                            }
                            let href = if evil_href {
                                format!("http://127.0.0.1:1/cal/{uid}.ics")
                            } else {
                                format!("/cal/{uid}.ics")
                            };
                            let esc = ics.replace('&', "&amp;").replace('<', "&lt;");
                            let _ = write!(
                                xml,
                                "<d:response><d:href>{href}</d:href><d:propstat><d:prop><c:calendar-data>{esc}</c:calendar-data></d:prop></d:propstat></d:response>"
                            );
                        }
                        xml.push_str("</d:multistatus>");
                        ("207 Multi-Status", xml)
                    }
                    _ => ("405 Method Not Allowed", String::new()),
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    Fake {
        port,
        events,
        seen_auth,
    }
}

fn connector(port: u16) -> Arc<CalendarConnector> {
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    store.set("calendar/home/password", "s3cret-pw").unwrap();
    let account = CalendarAccount {
        name: "home".into(),
        url: format!("http://127.0.0.1:{port}/cal"),
        username: "me".into(),
        password: SecretRef::new("calendar/home/password").unwrap(),
        timeout_secs: 5,
    };
    let (c, problems) = CalendarConnector::new(vec![account], &store);
    assert!(problems.is_empty(), "{problems:?}");
    Arc::new(c)
}

async fn call(c: &Arc<CalendarConnector>, name: &str, input: Value) -> Result<Value, ToolError> {
    let reg = ToolRegistry::new();
    assert_eq!(register(&reg, c), 3);
    let out = reg
        .get(name)
        .unwrap()
        .call(&ToolContext::default(), input)
        .await?;
    match out.content.into_iter().next() {
        Some(ContentPart::Json { data }) => Ok(data),
        other => panic!("unexpected output {other:?}"),
    }
}

#[tokio::test]
async fn create_list_and_delete_round_trip_with_the_stored_password() {
    let f = fake(false).await;
    let c = connector(f.port);
    let made = call(
        &c,
        "calendar_create_event",
        json!({"summary":"Dentist, 2nd floor; bring card","start":"2026-11-20T09:00:00+01:00",
               "end":"2026-11-20T10:00:00+01:00","location":"Main St","description":"line1\nline2"}),
    )
    .await
    .unwrap();
    let uid = made["uid"].as_str().unwrap().to_owned();
    // The ics on the wire is valid and escaped, and invites nobody.
    let raw = f.events.lock().unwrap().values().next().unwrap().clone();
    assert!(
        raw.contains("SUMMARY:Dentist\\, 2nd floor\\; bring card"),
        "{raw}"
    );
    assert!(raw.contains("DTSTART:20261120T080000Z"), "{raw}");
    assert!(!raw.contains("ATTENDEE"));

    let listed = call(
        &c,
        "calendar_list_events",
        json!({"from":"2026-11-20","to":"2026-11-21"}),
    )
    .await
    .unwrap();
    assert_eq!(listed["untrusted_external_content"], true);
    let ev = &listed["events"][0];
    assert_eq!(ev["uid"], uid);
    assert_eq!(ev["summary"], "Dentist, 2nd floor; bring card");
    assert_eq!(ev["start"], "2026-11-20T08:00:00Z");
    assert_eq!(ev["description"], "line1\nline2");

    let del = call(&c, "calendar_delete_event", json!({"uid": uid}))
        .await
        .unwrap();
    assert_eq!(del["deleted"], true);
    assert!(f.events.lock().unwrap().is_empty());
    let again = call(&c, "calendar_delete_event", json!({"uid": uid}))
        .await
        .unwrap();
    assert_eq!(again["deleted"], false);
    // Basic auth carried the secret-store value (base64 of me:s3cret-pw).
    assert!(
        f.seen_auth
            .lock()
            .unwrap()
            .iter()
            .all(|l| l.ends_with("bWU6czNjcmV0LXB3"))
    );
}

#[tokio::test]
async fn delete_refuses_an_href_on_another_host() {
    let f = fake(true).await;
    let c = connector(f.port);
    let made = call(
        &c,
        "calendar_create_event",
        json!({"summary":"x","start":"2026-11-20","end":"2026-11-21"}),
    )
    .await
    .unwrap();
    let err = call(&c, "calendar_delete_event", json!({"uid": made["uid"]}))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("outside the configured calendar"),
        "{err}"
    );
    assert_eq!(f.events.lock().unwrap().len(), 1);
}

#[test]
fn plain_http_to_a_remote_host_is_refused_and_effects_are_gated() {
    let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
    let account = CalendarAccount {
        name: "x".into(),
        url: "http://calendar.example.com/dav/".into(),
        username: "u".into(),
        password: SecretRef::new("calendar/x/password").unwrap(),
        timeout_secs: 5,
    };
    let (c, problems) = CalendarConnector::new(vec![account], &store);
    assert_eq!(c.names().len(), 0);
    assert!(problems[0].contains("must be https"), "{problems:?}");

    let c = Arc::new(c);
    let specs: Vec<_> = tools(&c).iter().map(|t| t.spec().clone()).collect();
    let effect = |n: &str| specs.iter().find(|s| s.name == n).unwrap().effect;
    assert_eq!(effect("calendar_list_events"), tool_registry::Effect::Read);
    assert_eq!(
        effect("calendar_delete_event"),
        tool_registry::Effect::Destructive
    );
}
