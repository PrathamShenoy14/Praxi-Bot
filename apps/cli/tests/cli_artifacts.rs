//! Exercise the artifact generators through the `bot` binary.

use std::fs;
use std::path::Path;
use std::process::Command;

fn bot(workspace: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bot"));
    command
        .current_dir(workspace)
        .env("BOT_DATA_DIR", workspace.join("unconfigured-data"));
    for name in [
        "BOT_API_TOKEN",
        "BOT_CATALOG_DIR",
        "BOT_PORT",
        "BOT_SECRETS_BACKEND",
        "BOT_URL",
    ] {
        command.env_remove(name);
    }
    command
}

#[test]
fn completions_write_shell_scripts_to_stdout() {
    let workspace = tempfile::tempdir().unwrap();
    for (shell, marker) in [
        ("bash", "complete "),
        ("zsh", "#compdef bot"),
        ("fish", "complete -c bot"),
        ("powershell", "Register-ArgumentCompleter"),
        ("elvish", "edit:completion:arg-completer"),
    ] {
        let output = bot(workspace.path())
            .args(["completions", shell])
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}: {output:?}");
        assert!(output.stderr.is_empty(), "{shell}: {output:?}");
        let script = String::from_utf8(output.stdout).unwrap();
        assert!(script.contains(marker), "missing {shell} completion syntax");
        assert!(script.contains("bot"), "missing command name for {shell}");
        assert!(script.contains("schedules"), "missing commands for {shell}");
    }
    assert!(!workspace.path().join("unconfigured-data").exists());
}

#[test]
fn completions_reject_missing_and_unsupported_shells() {
    let workspace = tempfile::tempdir().unwrap();
    for args in [
        ["completions"].as_slice(),
        ["completions", "unsupported"].as_slice(),
    ] {
        let output = bot(workspace.path()).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
        assert!(!output.stderr.is_empty(), "{args:?}: {output:?}");
    }
}

#[test]
fn man_pages_create_root_and_nested_section_one_pages() {
    let workspace = tempfile::tempdir().unwrap();
    let out_dir = workspace.path().join("docs").join("man1");
    let output = bot(workspace.path())
        .arg("man-pages")
        .arg(&out_dir)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");

    for (filename, text) in [
        ("bot.1", "Praxi"),
        ("bot-schedules-add.1", "Add a schedule"),
        ("bot-host-add.1", "Install Praxi"),
        ("bot-channels-telegram-setup.1", "BotFather"),
        ("bot-schedules-list.1", "List schedules"),
        ("bot-secrets-list.1", "secret"),
    ] {
        let page = fs::read_to_string(out_dir.join(filename))
            .unwrap_or_else(|error| panic!("could not read {filename} ({error})"));
        assert!(
            page.lines().any(|line| line.starts_with(".TH ")
                && line
                    .split_ascii_whitespace()
                    .nth(2)
                    .is_some_and(|section| section.trim_matches('"') == "1")),
            "missing section 1 header in {filename}"
        );
        assert!(page.contains(".SH NAME"), "missing NAME in {filename}");
        assert!(
            page.contains(".SH SYNOPSIS"),
            "missing SYNOPSIS in {filename}"
        );
        assert!(page.contains(text), "missing help text in {filename}");
    }
    assert!(!workspace.path().join("unconfigured-data").exists());
}

#[test]
fn man_pages_report_a_file_in_place_of_a_directory() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("file");
    fs::write(&file, b"").unwrap();
    let output = bot(workspace.path())
        .arg("man-pages")
        .arg(file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!output.stderr.is_empty(), "{output:?}");
}
