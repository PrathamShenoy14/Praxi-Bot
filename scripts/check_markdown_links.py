#!/usr/bin/env python3
"""Fail on broken local file links or heading anchors in tracked Markdown.

Run from the repository root:

    python scripts/check_markdown_links.py
"""
import html
import os
import re
import subprocess
import sys
import unicodedata
from urllib.parse import unquote

# [text](target) and [text](target "title"), including image paths.
LINK = re.compile(r"\[[^\]]*\]\(\s*<?([^)\s>]+)>?(?:\s+\"[^\"]*\")?\s*\)")
SCHEME = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")
CODE_SPAN = r"(?<!`)(?P<ticks>`+)(?!`)(?P<code>.*?)(?<!`)(?P=ticks)(?!`)"
INLINE_CODE = re.compile(CODE_SPAN)

ATX_HEADING = re.compile(r"^ {0,3}#{1,6}(?=[ \t]|$)[ \t]*(.*)$")
SETEXT_UNDERLINE = re.compile(r"^ {0,3}(?:=+|-+)[ \t]*$")
PARAGRAPH_BOUNDARY = re.compile(
    r"^(?:[ \t]*$| {4}|\t"  # Blank lines and indented code.
    r"| {0,3}(?:>|<|(?:[-+*]|[0-9]{1,9}[.)])(?:[ \t]|$)"  # Quotes, HTML blocks and lists.
    r"|([*_-])(?:[ \t]*\1){2,}[ \t]*$))"  # Thematic breaks.
)
HEADING_INLINE = re.compile(
    rf"{CODE_SPAN}"
    r"|!?\[(?P<label>[^\]]*)\]\([^)]*\)"
    r"|(?<!\w)(?P<marker>_{1,3})(?![_\s])(?P<emphasis>.+?)(?<![_\s])(?P=marker)(?!\w)"
    r"|</?[a-zA-Z][\w-]*(?:\s+[^<>]*)?/?>"
    r"|(?P<entity>&(?:#[xX][\da-fA-F]+|#\d+|[a-zA-Z][a-zA-Z\d]+);)"
)


def markdown_lines(path):
    """Yield physical line numbers and text outside fenced code blocks."""
    opening = ""
    with open(path, encoding="utf-8") as source:
        for number, line in enumerate(source, 1):
            line = line.rstrip("\n")
            match = FENCE.match(line)
            if match:
                marker, tail = match.groups()
                if opening:
                    if marker.startswith(opening) and not tail.strip():
                        opening = ""
                    continue
                if marker[0] == "~" or "`" not in tail:
                    opening = marker
                    continue
            if not opening:
                yield number, line


def markdown_headings(path):
    """Yield ATX and Setext heading text in document order."""
    paragraph = []
    previous = 0
    for number, line in markdown_lines(path):
        if number != previous + 1:
            paragraph.clear()
        previous = number
        atx = ATX_HEADING.fullmatch(line)
        if atx:
            yield re.sub(r"(?:^|[ \t]+)#+[ \t]*$", "", atx[1])
        elif SETEXT_UNDERLINE.fullmatch(line):
            if paragraph:
                yield " ".join(paragraph)
        elif not PARAGRAPH_BOUNDARY.match(line):
            paragraph.append(line.strip())
            continue
        paragraph.clear()


def visible_heading_text(markdown):
    def replace_markup(match):
        code = match["code"]
        if code is not None:
            if code.startswith(" ") and code.endswith(" ") and code.strip(" "):
                return code[1:-1]
            return code
        if match["label"] is not None:
            return visible_heading_text(match["label"])
        if match["emphasis"] is not None:
            return visible_heading_text(match["emphasis"])
        if match["entity"] is not None:
            return html.unescape(match["entity"])
        return ""

    return HEADING_INLINE.sub(replace_markup, markdown)


def heading_anchors(path):
    # The value for each ID is the last suffix used when that ID was the base.
    used = {}
    for heading in markdown_headings(path):
        text = visible_heading_text(heading).strip().lower()
        base = "".join(
            char for char in text
            if char in " -_" or unicodedata.category(char).startswith(("L", "M", "N"))
        ).replace(" ", "-")
        anchor = base
        while anchor in used:
            used[base] += 1
            anchor = f"{base}-{used[base]}"
        used[anchor] = 0
    return used.keys()


def main():
    root = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, encoding="utf-8", check=True
    ).stdout.strip()
    files = subprocess.run(
        ["git", "ls-files", "-z", "*.md"], capture_output=True, text=True, encoding="utf-8", check=True
    ).stdout.split("\0")

    anchors = {}
    bad = []
    for rel in filter(None, files):
        path = os.path.normpath(os.path.join(root, rel))
        for number, line in markdown_lines(path):
            for match in LINK.finditer(INLINE_CODE.sub("", line)):
                target = match[1]
                if SCHEME.match(target):
                    continue
                relative, _, fragment = target.partition("#")
                relative = relative.split("?", 1)[0]
                if relative.startswith("/"):
                    resolved = os.path.normpath(os.path.join(root, relative.lstrip("/")))
                elif relative:
                    resolved = os.path.normpath(os.path.join(os.path.dirname(path), relative))
                else:
                    resolved = path

                if not os.path.exists(resolved):
                    bad.append(f"{rel}:{number}: broken link -> {target}")
                elif fragment and resolved.lower().endswith(".md") and os.path.isfile(resolved):
                    if resolved not in anchors:
                        anchors[resolved] = heading_anchors(resolved)
                    if unquote(fragment) not in anchors[resolved]:
                        bad.append(f"{rel}:{number}: broken anchor -> {target}")

    if bad:
        print("Broken relative Markdown links:")
        print("\n".join(bad))
        print(f"\n{len(bad)} broken link(s). Fix the path or anchor, or remove the link.")
        return 1
    print("All relative Markdown links resolve.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
