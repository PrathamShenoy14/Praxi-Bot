//! Native validation and handoff for desktop protocol activations.

use std::ffi::OsStr;

use parking_lot::Mutex;
use serde::Serialize;

const MAX_URI_BYTES: usize = "praxi://chat/".len() + 36;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Activation {
    Bot { id: String },
    Chat { id: String },
    Run { id: String },
    Invalid,
}

/// The pending value survives a missed wake-up and is consumed at most once.
pub(crate) struct Pending(Mutex<Option<Activation>>);

impl Pending {
    pub(crate) fn new(cold: Option<Activation>) -> Self {
        Self(Mutex::new(cold))
    }

    pub(crate) fn replace(&self, activation: Activation) {
        *self.0.lock() = Some(activation);
    }

    pub(crate) fn take(&self) -> Option<Activation> {
        self.0.lock().take()
    }

    pub(crate) fn has_pending(&self) -> bool {
        self.0.lock().is_some()
    }
}

pub(crate) fn from_argv(argv: &[impl AsRef<OsStr>]) -> Option<Activation> {
    from_inputs(
        argv.iter()
            .skip(1)
            .map(|arg| arg.as_ref().as_encoded_bytes()),
    )
}

/// A batch can open a destination only when it contains a single input.
pub(crate) fn from_inputs<'a>(mut inputs: impl Iterator<Item = &'a [u8]>) -> Option<Activation> {
    let first = inputs.next()?;
    if let Some(second) = inputs.next() {
        return [first, second]
            .into_iter()
            .chain(inputs)
            .any(is_praxi_attempt)
            .then_some(Activation::Invalid);
    }
    is_praxi_attempt(first).then(|| parse(first).unwrap_or(Activation::Invalid))
}

fn is_praxi_attempt(input: &[u8]) -> bool {
    // Recognize the scheme even if malformed content follows it. Inspect
    // only the prefix, and give the parser the original bytes.
    let prefix = input.get(..MAX_URI_BYTES).unwrap_or(input);
    prefix.utf8_chunks().next().is_some_and(|chunk| {
        chunk
            .valid()
            .trim_start()
            .as_bytes()
            .first_chunk::<6>()
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"praxi:"))
    })
}

fn parse(input: &[u8]) -> Option<Activation> {
    if input.len() > MAX_URI_BYTES {
        return None;
    }
    let path = std::str::from_utf8(input).ok()?.strip_prefix("praxi://")?;
    let (kind, id) = path.split_once('/')?;
    if id.len() != 36
        || !id.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
        })
    {
        return None;
    }
    match kind {
        "bot" => Some(Activation::Bot { id: id.to_owned() }),
        "chat" => Some(Activation::Chat { id: id.to_owned() }),
        "run" => Some(Activation::Run { id: id.to_owned() }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Activation, Pending, from_argv, from_inputs};

    const ID: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn launch(input: &str) -> Option<Activation> {
        from_argv(&["bot-desktop", input])
    }

    #[test]
    fn accepts_three_destinations_and_any_canonical_uuid_version() {
        for (kind, expected) in [
            ("bot", Activation::Bot { id: ID.to_owned() }),
            ("chat", Activation::Chat { id: ID.to_owned() }),
            ("run", Activation::Run { id: ID.to_owned() }),
        ] {
            assert_eq!(launch(&format!("praxi://{kind}/{ID}")), Some(expected));
        }
        for id in [
            "00000000-0000-0000-0000-000000000000",
            "123e4567-e89b-42d3-a456-426614174000",
            "123e4567-e89b-72d3-a456-426614174000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
        ] {
            assert_eq!(
                launch(&format!("praxi://bot/{id}")),
                Some(Activation::Bot { id: id.to_owned() })
            );
        }
    }

    #[test]
    fn recognizes_but_rejects_malformed_praxi_attempts() {
        for input in [
            "praxi:".to_owned(),
            format!("Praxi://bot/{ID}"),
            format!("praxi://BOT/{ID}"),
            format!("praxi://bot/{}", ID.to_uppercase()),
            format!("praxi://bot/{}", ID.replace('-', "")),
            format!("praxi://bot/{}", ID.replacen('1', "g", 1)),
            format!("praxi://bot/{}", ID.replacen('1', "%31", 1)),
            format!("praxi://bot/{}", ID.replacen('1', "１", 1)),
            format!("praxi://bot:99/{ID}"),
            format!("praxi://user@bot/{ID}"),
            format!("praxi://@bot/{ID}"),
            format!("praxi://bot/{ID}?"),
            format!("praxi://bot/{ID}#"),
            format!("praxi://bot/{ID}/"),
            format!("praxi://bot/extra/{ID}"),
            format!("praxi://bot/extra/../{ID}"),
            format!("praxi:///bot/{ID}"),
            format!("praxi:/bot/{ID}"),
            format!("praxi://settings/{ID}"),
            format!(" praxi://bot/{ID}"),
            format!("\u{2003}praxi://bot/{ID}"),
            format!("praxi://bot/{ID}\n"),
            format!("praxi://bot/{ID}|"),
            format!("praxi://chat/{}", "a".repeat(2048)),
        ] {
            assert_eq!(launch(&input), Some(Activation::Invalid), "{input:?}");
        }
        assert_eq!(
            from_inputs(std::iter::once(b"praxi://bot/\xff".as_slice())),
            Some(Activation::Invalid)
        );
    }

    #[test]
    fn only_one_launch_argument_can_supply_a_destination() {
        let valid = format!("praxi://run/{ID}");
        assert_eq!(from_argv(&[valid.as_str()]), None);
        assert_eq!(
            from_argv(&["bot-desktop", "--dev", "other", &valid]),
            Some(Activation::Invalid)
        );
        assert_eq!(
            from_argv(&["bot-desktop", &valid, &valid]),
            Some(Activation::Invalid)
        );
        assert_eq!(from_argv(&["bot-desktop", "--dev", "other"]), None);
        for input in ["praxi", "praxis:bot", "https://praxi:42", "--open=praxi:"] {
            assert_eq!(launch(input), None);
        }
    }

    #[test]
    fn pending_keeps_the_latest_attempt_until_taken() {
        let bot = format!("praxi://bot/{ID}");
        let run = format!("praxi://run/{ID}");
        let pending = Pending::new(launch(&bot));
        assert!(pending.has_pending());
        pending.replace(launch("praxi://not-a-route").unwrap());
        assert_eq!(pending.take(), Some(Activation::Invalid));
        assert_eq!(pending.take(), None);
        pending.replace(launch(&run).unwrap());
        assert_eq!(pending.take(), Some(Activation::Run { id: ID.to_owned() }));
        assert!(!pending.has_pending());
    }

    #[test]
    fn serializes_only_the_destination_or_the_generic_invalid_result() {
        assert_eq!(
            serde_json::to_value(launch(&format!("praxi://chat/{ID}"))).unwrap(),
            serde_json::json!({"kind": "chat", "id": ID})
        );
        assert_eq!(
            serde_json::to_value(launch("praxi://bot/raw-input")).unwrap(),
            serde_json::json!({"kind": "invalid"})
        );
        assert_eq!(
            serde_json::to_value(launch("--dev")).unwrap(),
            serde_json::Value::Null
        );
    }
}
