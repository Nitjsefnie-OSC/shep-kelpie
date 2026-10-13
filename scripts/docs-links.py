#!/usr/bin/env python3
"""The README and docs/guide/ are what the shep website renders, and it imports
every page a relative link reaches, so their links hold together:

- every relative link names a file in the repo;
- every anchor, on the same page or another, names a heading that exists
  (GitHub's rule turns a heading into its anchor);
- every page under docs/guide/ is reachable from README.md by relative links;
- no relative link leaves README.md and docs/guide/: a file outside them is
  linked by an absolute GitHub URL.

External links are never fetched. Pass a folder to check one other than the
repo this script is in. Setext headings (underlined with === or ---) are not
read, and a link whose anchor names a non-Markdown file is not checked past the
file. Prints each failure as page:line, the link and what is wrong.
"""
import os
import posixpath
import re
import sys
import unicodedata
from urllib.parse import unquote

FENCE = re.compile(r"^\s*(`{3,}|~{3,})")
ATX = re.compile(r"^ {0,3}#{1,6}[ \t]+(.*?)(?:[ \t]+#+)?[ \t]*$")
CODE_SPAN = re.compile(r"(`+)(?:.|\n)*?(?<!`)\1(?!`)")
INLINE_LINK = re.compile(
    r"\[(?:[^\[\]]|\[[^\]]*\])*\]\(\s*(<[^>\n]*>|(?:[^()\s]|\([^()\s]*\))*)"
    r"(?:\s+(?:\"[^\"]*\"|'[^']*'))?\s*\)"
)
REFERENCE_DEFINITION = re.compile(r"^ {0,3}\[[^\]\n]+\]:[ \t]*(<[^>\n]*>|\S+)", re.M)
HTML_LINK = re.compile(r"""<(?:a|img)\b[^>]*?\b(?:href|src)=["']([^"']*)["']""", re.I)
HTML_ID = re.compile(r"""\b(?:id|name)=["']([^"']+)["']""")
SCHEME = re.compile(r"^(?:[a-zA-Z][a-zA-Z0-9+.-]*:|//)")


def blank_code(text):
    """The text with fenced blocks and inline code spans blanked out, keeping
    every line where it was."""
    out, fence = [], None
    for line in text.split("\n"):
        match = FENCE.match(line)
        if fence is None and match:
            fence = match.group(1)
            out.append("")
        elif fence is not None:
            if match and match.group(1)[0] == fence[0] and len(match.group(1)) >= len(fence):
                fence = None
            out.append("")
        else:
            out.append(line)
    text = "\n".join(out)
    return CODE_SPAN.sub(lambda m: re.sub(r"[^\n]", " ", m.group(0)), text)


def links(text):
    """Every (line, target) relative or not, outside code."""
    text = blank_code(text)
    found = []
    for pattern in (INLINE_LINK, REFERENCE_DEFINITION, HTML_LINK):
        for match in pattern.finditer(text):
            target = match.group(1).strip()
            if target.startswith("<") and target.endswith(">"):
                target = target[1:-1]
            line = text.count("\n", 0, match.start(1)) + 1
            found.append((line, target))
    return sorted(found)


def heading_text(raw):
    """What a heading renders as, as plain text."""
    raw = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", raw)
    raw = re.sub(r"!?\[([^\]]*)\]\[[^\]]*\]", r"\1", raw)
    raw = re.sub(r"<[^>]*>", "", raw)
    raw = re.sub(r"(`+)(.*?)\1", r"\2", raw)
    raw = raw.replace("*", "")
    return re.sub(r"(?<!\w)_+(.+?)_+(?!\w)", r"\1", raw)


def slug(text):
    """GitHub's anchor for a heading: lower case, only letters, marks,
    numbers, underscores, hyphens and spaces kept, each space a hyphen."""
    kept = [
        c
        for c in text.lower().strip()
        if c in "- " or unicodedata.category(c)[0] in "LMN" or unicodedata.category(c) == "Pc"
    ]
    return "".join(kept).replace(" ", "-")


def anchors(text):
    """Every anchor a page has: its headings, a repeat getting -1, -2, and
    any id or name an HTML tag gives."""
    seen, names = {}, set()
    body = []
    fence = None
    for line in text.split("\n"):
        match = FENCE.match(line)
        if fence is None and match:
            fence = match.group(1)
        elif fence is not None:
            if match and match.group(1)[0] == fence[0] and len(match.group(1)) >= len(fence):
                fence = None
        else:
            body.append(line)
    for line in body:
        heading = ATX.match(line)
        if heading:
            base = slug(heading_text(heading.group(1)))
            count = seen.get(base, 0)
            seen[base] = count + 1
            names.add(base if count == 0 else f"{base}-{count}")
        names.update(HTML_ID.findall(line))
    return names


def main():
    root = os.path.abspath(sys.argv[1]) if len(sys.argv) > 1 else os.path.dirname(
        os.path.dirname(os.path.abspath(__file__))
    )
    guide = []
    for folder, _, files in os.walk(os.path.join(root, "docs", "guide")):
        for name in files:
            if name.endswith(".md"):
                guide.append(os.path.relpath(os.path.join(folder, name), root).replace(os.sep, "/"))
    pages = sorted(["README.md"] + guide)

    def in_scope(path):
        return path == "README.md" or path.startswith("docs/guide/")

    def read(path):
        with open(os.path.join(root, path), encoding="utf-8") as f:
            return f.read()

    failures = []
    reached = {"README.md"}
    queue = ["README.md"]
    anchor_cache = {}
    while queue:
        page = queue.pop()
        for line, link in links(read(page)):
            if SCHEME.match(link):
                continue
            path, _, fragment = link.partition("#")
            path = unquote(path.partition("?")[0])
            fragment = unquote(fragment)

            def fail(why):
                failures.append(f"{page}:{line}: [{link}]: {why}")

            if path == "":
                target = page
            elif path.startswith("/"):
                target = posixpath.normpath(path.lstrip("/"))
            else:
                target = posixpath.normpath(posixpath.join(posixpath.dirname(page), path))
            if target == ".." or target.startswith("../"):
                fail("leads out of the repo")
                continue
            full = os.path.join(root, target)
            if not os.path.exists(full):
                fail("no such file")
                continue
            if os.path.isdir(full):
                fail("names a folder, not a file")
                continue
            if not in_scope(target):
                fail(f"leads outside README.md and docs/guide/ to {target}: link it by an absolute GitHub URL")
                continue
            if not target.endswith(".md"):
                continue
            if target not in reached:
                reached.add(target)
                queue.append(target)
            if fragment:
                if target not in anchor_cache:
                    anchor_cache[target] = anchors(read(target))
                if fragment not in anchor_cache[target]:
                    fail(f"no heading makes the anchor #{fragment} in {target}")

    for page in guide:
        if page not in reached:
            failures.append(f"{page}: not reachable from README.md by relative links")

    for failure in sorted(set(failures)):
        print(failure)
    if failures:
        print(f"{len(set(failures))} problem(s) in README.md and docs/guide/")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
