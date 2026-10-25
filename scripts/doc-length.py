#!/usr/bin/env python3
"""IR-47 on this machine and in CI: the line limits for comment blocks in the
tracked `.rs` files.

A block is a run of whole-line comments of one kind. Every line counts, blank
ones too, except fenced code and `# Example` sections of doc comments and the
blank line after a `///` summary. The limits:

- `///`: at most 6 lines after the summary (its first line), and at most 12
  counting the `# Errors` section;
- `//!`: at most 10 lines;
- `//`: at most 4 lines in a row.

Only lines are counted: sentence length, history and review arguments stay with
whoever reads the code.
"""
import os
import re
import subprocess
import sys

BASELINE = ".github/doc-length-baseline.txt"
# Per kind, the limit on each size the kind measures.
LIMITS = {"///": (6, 12), "//!": (10,), "//": (4,)}
FENCE = re.compile(r"\s*(```|~~~)")
HEADING = re.compile(r"#+\s*(.*?)\s*$")


def repo_root():
    out = subprocess.run(
        ["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True
    )
    return out.stdout.strip()


def rust_files(root):
    out = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "*.rs"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    return [path for path in out.stdout.split("\0") if path]


def classify(line):
    """The kind of a whole-line comment and its text, or None for anything else."""
    stripped = line.lstrip()
    for kind in ("///", "//!", "//"):
        if stripped.startswith(kind):
            if kind == "///" and stripped.startswith("////"):
                continue
            return kind, stripped[len(kind):].strip()
    return None


def blocks(text):
    """Every run of whole-line comments of one kind, as (first line number, kind,
    first line as written, the text of each line)."""
    found, current = [], None
    for number, line in enumerate(text.split("\n"), 1):
        parsed = classify(line)
        if parsed and current and parsed[0] == current[1]:
            current[3].append(parsed[1])
        else:
            if parsed:
                current = [number, parsed[0], line.strip(), [parsed[1]]]
                found.append(current)
            else:
                current = None
    return [tuple(block) for block in found]


def counted(texts):
    """The lines of a doc comment that count, as (section, text) pairs: the
    section is "errors" under `# Errors` and "body" anywhere else. Fenced code
    and `# Example` sections are left out."""
    section, fenced, out = "body", False, []
    for text in texts:
        if FENCE.match(text):
            fenced = not fenced
            continue
        if fenced:
            continue
        heading = HEADING.match(text) if text.startswith("#") else None
        if heading:
            name = heading.group(1).lower()
            section = "errors" if name == "errors" else "skip" if name in ("example", "examples") else "body"
        if section != "skip":
            out.append((section, text))
    return out


def sizes(kind, texts):
    """What a block is measured by, in the order of LIMITS[kind]."""
    lines = counted(texts)
    if kind != "///":
        return (len(lines),)
    # The summary is the first line, and a blank line after it only separates.
    lines = lines[2:] if len(lines) > 1 and not lines[1][1] else lines[1:]
    body = len([line for line in lines if line[0] == "body"])
    return (body, len(lines))


def describe(kind, size):
    limits = LIMITS[kind]
    if kind == "//":
        return f"{size[0]} lines in a row, at most {limits[0]}"
    if kind == "//!":
        return f"{size[0]} lines, at most {limits[0]}"
    over = []
    if size[0] > limits[0]:
        over.append(f"{size[0]} lines after the summary, at most {limits[0]}")
    if size[1] > limits[1]:
        over.append(f"{size[1]} lines after the summary counting # Errors, at most {limits[1]}")
    return "; ".join(over)


def is_over(kind, size):
    return any(have > limit for have, limit in zip(size, LIMITS[kind]))


def over_limit(root):
    """Every block over its limit, as (path, line number, kind, first line, size)."""
    found = []
    for path in rust_files(root):
        try:
            with open(f"{root}/{path}") as handle:
                text = handle.read()
        except FileNotFoundError:
            continue
        for number, kind, first, texts in blocks(text):
            size = sizes(kind, texts)
            if is_over(kind, size):
                found.append((path, number, kind, first, size))
    return found


def parse_baseline(text):
    """The baseline as {(path, first line): [sizes, in file order]}, with the
    baseline's line number of each entry."""
    entries = {}
    for number, line in enumerate(text.split("\n"), 1):
        if line:
            path, size, first = line.split("\t", 2)
            entries.setdefault((path, first), []).append((number, tuple(map(int, size.split(",")))))
    return entries


def format_baseline(found):
    return "".join(
        f"{path}\t{','.join(map(str, size))}\t{first}\n"
        for path, _, _, first, size in sorted(found, key=lambda block: (block[0], block[1]))
    )


def check(root, baseline_path):
    """The failures, as lines to print."""
    try:
        with open(baseline_path) as handle:
            listed = parse_baseline(handle.read())
    except FileNotFoundError:
        listed = {}
    failures = []
    for path, number, kind, first, size in over_limit(root):
        entries = listed.get((path, first))
        if not entries:
            failures.append(f"{path}:{number}: {describe(kind, size)}")
            continue
        _, was = entries.pop(0)
        if any(have > before for have, before in zip(size, was)):
            failures.append(
                f"{path}:{number}: {describe(kind, size)}; it was {','.join(map(str, was))} "
                f"when baselined and a listed block may only shrink"
            )
    for (path, first), entries in listed.items():
        for number, _ in entries:
            failures.append(
                f"{BASELINE}:{number}: {path} `{first}` fits its limit or is gone; take it off the baseline"
            )
    return failures


def main():
    root = repo_root()
    baseline_path = f"{root}/{BASELINE}"
    if sys.argv[1:] == ["--update"]:
        os.makedirs(os.path.dirname(baseline_path), exist_ok=True)
        with open(baseline_path, "w") as handle:
            handle.write(format_baseline(over_limit(root)))
        return 0
    failures = check(root, baseline_path)
    for failure in failures:
        print(failure)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
