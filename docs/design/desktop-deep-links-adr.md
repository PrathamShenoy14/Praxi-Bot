# ADR: Desktop deep links

Status: proposed in [issue #234](https://github.com/emreocell/Praxi-Bot/issues/234).

## Context

A browser, document, or other application can open a desktop link. Treat
every activation as untrusted input, including an argument forwarded by
another Praxi-Bot process.

## Decision

- Accept `praxi://bot/<uuid>`, `praxi://chat/<uuid>`, and
  `praxi://run/<uuid>`. The scheme, destination, and hyphenated hexadecimal
  UUID are lowercase. All UUID versions are allowed. The maximum is 49
  ASCII bytes. The grammar has no fields for query strings, fragments,
  extra path segments, percent escapes, or text.
- Validate in the Rust shell before notifying the main window. Windows
  and Linux use the launch argument as delivered. macOS supplies an
  already-parsed URL, so validation applies to that representation.
  Only a typed destination or generic invalid result reaches the UI.
  A single pending slot keeps the latest activation while the UI starts.
  The main window drains it after subscribing to the payload-free signal.
- Resolve against the selected computer through the desktop's existing
  authenticated read paths. A link cannot choose a host, switch computers,
  or cause a lookup on another host. Ignore lookup results if the computer
  changes while they are in flight.
- Open a bot's usual landing view, the exact chat, or the exact run's
  redacted timeline. A run without available events still opens with the
  saved run status and timestamps. A link has no authority to send
  messages, launch bots or tools, decide approvals, install skills,
  change settings, export data, or reveal secrets.
- Preserve unsaved and in-progress edits. An invalid, unavailable, or
  blocked destination gives local feedback. Error messages and logs
  exclude the supplied URL.

## Platform registration

Tauri's desktop scheme configuration supplies the package metadata. The
per-user Windows NSIS installer registers the handler and removes it on
uninstall if it still points to that installation. The existing instance
receives forwarded links before a second app can start its daemon connection.

Linux desktop packages need an installed desktop entry that declares
`x-scheme-handler/praxi` and forwards the URL to the executable. Forwarding to
a running instance uses the desktop D-Bus session. A portable build without
desktop integration has no OS association. macOS packages declare the scheme
in the app bundle's `CFBundleURLTypes`. Verify the installed handlers and
their removal when the desktop bundles in
[issue #175](https://github.com/emreocell/Praxi-Bot/issues/175) are available.
