"""Exercise the Markdown link checker in a temporary Git checkout."""

from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
from textwrap import dedent
import unittest


CHECKER = Path(__file__).resolve().parents[1] / "check_markdown_links.py"


class MarkdownLinkCheckerTests(unittest.TestCase):
    def run_checker(self, files):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q"], cwd=root, check=True, capture_output=True)
            for name, content in files.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(dedent(content).lstrip("\n"), encoding="utf-8")
            subprocess.run(["git", "add", "--", "."], cwd=root, check=True, capture_output=True)
            return subprocess.run(
                [sys.executable, str(CHECKER)], cwd=root, capture_output=True,
                text=True, encoding="utf-8",
            )

    def assert_diagnostics(self, result, *expected):
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertEqual(
            [line for line in result.stdout.splitlines() if " -> " in line],
            list(expected),
        )

    def test_resolves_same_file_and_cross_file_anchors(self):
        result = self.run_checker({
            "README.md": """
                [Later heading](#use-bold-and-file)
                [Relative link](docs/guide.md#ordinary-heading)

                ## Use **Bold** and `<FILE>` ##
            """,
            "docs/guide.md": """
                [Root link](/README.md#use-bold-and-file)
                [Encoded fragment](#caf%C3%A9-api_v1-quick-start)
                [First repeat](#repeat)
                [Explicit suffix](#repeat-1)
                [Second repeat](#repeat-2)
                [Third repeat](#repeat-3)

                Ordinary Heading
                ----------------

                # _Café_ API_v1 [Quick Start](https://example.invalid/docs)
                # Repeat-1
                # Repeat
                # Repeat
                # Repeat
            """,
        })
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_reports_missing_anchors_and_keeps_path_checks(self):
        result = self.run_checker({
            "README.md": """
                # Welcome
                [Missing here](#absent)
                [Existing heading](guide.md?mode=print#overview)
                [Missing there](/guide.md?mode=print#absent)
                [Missing file](missing.md#absent)
                [Asset](diagram.svg#unverified)
                [Website](https://example.invalid/missing.md#absent)
                [Top](#)
                [Other top](guide.md#)
            """,
            "guide.md": "# Overview\n",
            "diagram.svg": "<svg />\n",
        })
        self.assert_diagnostics(
            result,
            "README.md:2: broken anchor -> #absent",
            "README.md:4: broken anchor -> /guide.md?mode=print#absent",
            "README.md:5: broken link -> missing.md#absent",
        )

    def test_ignores_headings_and_links_inside_fences(self):
        result = self.run_checker({
            "README.md": """
                ````markdown
                ~~~~
                # Hidden
                    ````
                [Fake link after indented marker](missing.md)
                ```
                [Fake link after short marker](missing.md)
                ````
                [Hidden heading](#hidden)
                [Visible heading](#visible)
                # Visible
            """,
        })
        self.assert_diagnostics(result, "README.md:9: broken anchor -> #hidden")


if __name__ == "__main__":
    unittest.main()
