#!/usr/bin/env python3
"""Tests for doc-length.py, through its command line: each test builds a small
git repo, runs the script in it and reads the exit code and the output."""
import os
import subprocess
import sys
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "doc-length.py")
BASELINE = ".github/doc-length-baseline.txt"


def lines(prefix, count, indent=""):
    return "".join(f"{indent}{prefix} line {n}\n" for n in range(1, count + 1))


class Repo:
    """A throwaway git repo with `.rs` files and an optional baseline."""

    def __init__(self, files, baseline=None):
        self.dir = tempfile.TemporaryDirectory()
        self.root = self.dir.name
        subprocess.run(["git", "init", "-q", self.root], check=True)
        for path, text in files.items():
            self.write(path, text)
        if baseline is not None:
            self.write(BASELINE, baseline)

    def write(self, path, text):
        full = os.path.join(self.root, path)
        os.makedirs(os.path.dirname(full), exist_ok=True)
        with open(full, "w") as handle:
            handle.write(text)

    def run(self, *args):
        return subprocess.run(
            [sys.executable, "-I", SCRIPT, *args],
            cwd=self.root,
            capture_output=True,
            text=True,
        )

    def baseline(self):
        with open(os.path.join(self.root, BASELINE)) as handle:
            return handle.read()


def check(files, baseline=None):
    repo = Repo(files, baseline)
    try:
        return repo.run()
    finally:
        repo.dir.cleanup()


class LineCommentLimit(unittest.TestCase):
    def test_four_lines_in_a_row_pass(self):
        result = check({"src/a.rs": lines("//", 4) + "fn a() {}\n"})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_blank_comment_lines_count(self):
        result = check({"src/a.rs": "// a\n//\n// b\n//\n// c\n"})
        self.assertEqual(result.returncode, 1)

    def test_a_line_of_code_ends_the_block(self):
        result = check({"src/a.rs": lines("//", 3) + "fn a() {}\n" + lines("//", 3)})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_example_headings_and_fences_do_not_exempt_a_line_comment(self):
        example = "// # Example\n" + lines("//", 8) + "fn a() {}\n"
        fenced = "// ```\n" + lines("//", 8) + "// ```\nfn b() {}\n"
        self.assertEqual(check({"src/a.rs": example}).returncode, 1)
        self.assertEqual(check({"src/a.rs": fenced}).returncode, 1)

    def test_five_lines_in_a_row_fail_naming_file_and_first_line(self):
        result = check({"src/a.rs": "fn a() {}\n\n" + lines("//", 5) + "fn b() {}\n"})
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs:3", result.stdout)


def doc(*body, summary="Summary."):
    return "".join(f"/// {text}\n" for text in (summary, *body)) + "pub fn a() {}\n"


def numbered(count, start=1):
    return [f"body {n}." for n in range(start, start + count)]


class OuterDocLimit(unittest.TestCase):
    def test_summary_and_six_body_lines_pass(self):
        result = check({"src/a.rs": doc(*numbered(6))})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_seven_body_lines_fail_naming_file_and_first_line(self):
        result = check({"src/a.rs": "use b;\n\n" + doc(*numbered(7))})
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs:3", result.stdout)

    def test_the_blank_line_after_the_summary_does_not_count(self):
        result = check({"src/a.rs": doc("", *numbered(6))})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_other_blank_lines_count(self):
        body = ["a.", "", "b.", "", "c.", "", "d."]
        result = check({"src/a.rs": doc(*body)})
        self.assertEqual(result.returncode, 1)

    def test_errors_section_raises_the_limit_to_twelve_in_all(self):
        errors = ["# Errors", *numbered(5, 100)]
        result = check({"src/a.rs": doc(*numbered(6), *errors)})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_thirteen_lines_after_the_summary_fail_even_with_errors(self):
        errors = ["# Errors", *numbered(6, 100)]
        result = check({"src/a.rs": doc(*numbered(6), *errors)})
        self.assertEqual(result.returncode, 1)

    def test_seven_body_lines_fail_even_when_an_errors_section_follows(self):
        result = check({"src/a.rs": doc(*numbered(7), "# Errors", "Never.")})
        self.assertEqual(result.returncode, 1)

    def test_fenced_code_does_not_count(self):
        code = ["```", *numbered(10, 200), "```"]
        result = check({"src/a.rs": doc(*numbered(6), *code)})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_example_section_does_not_count(self):
        example = ["# Example", *numbered(10, 200)]
        result = check({"src/a.rs": doc(*numbered(6), *example)})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_a_section_after_an_example_counts_again(self):
        after = ["# Example", *numbered(10, 200), "# Panics", *numbered(2, 300)]
        result = check({"src/a.rs": doc(*numbered(5), *after)})
        self.assertEqual(result.returncode, 1)

    def test_indented_doc_comments_are_checked(self):
        text = "impl A {\n" + "".join(f"    /// {t}\n" for t in ["S.", *numbered(7)]) + "    fn a() {}\n}\n"
        result = check({"src/a.rs": text})
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs:2", result.stdout)


class ModuleDocLimit(unittest.TestCase):
    def test_ten_lines_pass(self):
        result = check({"src/a.rs": lines("//!", 10) + "\nfn a() {}\n"})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_eleven_lines_fail_naming_file_and_first_line(self):
        result = check({"src/a.rs": lines("//!", 11) + "\nfn a() {}\n"})
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs:1", result.stdout)

    def test_fenced_code_does_not_count(self):
        text = "//! Summary.\n//! ```text\n" + lines("//!", 12) + "//! ```\n"
        result = check({"src/a.rs": text})
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_a_module_doc_and_a_comment_after_it_are_separate_blocks(self):
        result = check({"src/a.rs": lines("//!", 3) + lines("//", 3)})
        self.assertEqual(result.returncode, 0, result.stdout)


class Baseline(unittest.TestCase):
    def setUp(self):
        self.long_comment = lines("//", 6) + "fn a() {}\n"
        self.repo = Repo({"src/a.rs": self.long_comment})
        self.addCleanup(self.repo.dir.cleanup)

    def baselined(self):
        updated = self.repo.run("--update")
        self.assertEqual(updated.returncode, 0, updated.stdout + updated.stderr)

    def test_a_block_over_its_limit_passes_while_it_is_on_the_baseline(self):
        self.baselined()
        result = self.repo.run()
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_the_baseline_names_the_file_and_the_first_line(self):
        self.baselined()
        self.assertIn("src/a.rs", self.repo.baseline())
        self.assertIn("// line 1", self.repo.baseline())

    def test_the_baseline_does_not_depend_on_line_numbers(self):
        self.baselined()
        self.repo.write("src/a.rs", "use b;\n\n" + self.long_comment)
        result = self.repo.run()
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_a_listed_block_may_shrink_and_stay_over(self):
        self.baselined()
        self.repo.write("src/a.rs", lines("//", 5) + "fn a() {}\n")
        result = self.repo.run()
        self.assertEqual(result.returncode, 0, result.stdout)

    def test_a_listed_block_may_not_grow(self):
        self.baselined()
        self.repo.write("src/a.rs", lines("//", 7) + "fn a() {}\n")
        result = self.repo.run()
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs:1", result.stdout)

    def test_a_new_block_over_its_limit_fails_beside_a_baselined_one(self):
        self.baselined()
        self.repo.write("src/a.rs", self.long_comment + "\n" + lines("//", 5, "    ") + "fn b() {}\n")
        result = self.repo.run()
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs:9", result.stdout)

    def test_a_listed_block_that_now_fits_fails_until_it_is_taken_off(self):
        self.baselined()
        self.repo.write("src/a.rs", lines("//", 4) + "fn a() {}\n")
        result = self.repo.run()
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs", result.stdout)
        self.assertIn("// line 1", result.stdout)
        self.repo.write(BASELINE, "")
        self.assertEqual(self.repo.run().returncode, 0)

    def test_a_listed_block_that_is_gone_fails_until_it_is_taken_off(self):
        self.baselined()
        self.repo.write("src/a.rs", "fn a() {}\n")
        result = self.repo.run()
        self.assertEqual(result.returncode, 1)
        self.assertIn("src/a.rs", result.stdout)

    def test_update_rewrites_the_baseline_to_what_is_over_today(self):
        self.baselined()
        self.repo.write("src/a.rs", lines("//", 4) + "fn a() {}\n")
        self.baselined()
        self.assertEqual(self.repo.baseline(), "")
        self.assertEqual(self.repo.run().returncode, 0)

    def test_two_listed_blocks_with_the_same_first_line_are_each_listed(self):
        self.repo.write("src/a.rs", self.long_comment + "\n" + self.long_comment)
        self.baselined()
        self.assertEqual(self.repo.run().returncode, 0)
        self.repo.write("src/a.rs", self.long_comment)
        self.assertEqual(self.repo.run().returncode, 1)


if __name__ == "__main__":
    unittest.main()
