#!/usr/bin/env python3
"""Prints the name of every workflow a pull request must trigger.

  expected.py [--jobs] --base BRANCH [--dir DIR] < changed-files

Reads the changed file paths (one per line) on stdin, and every workflow in
DIR (default .github/workflows). A workflow is expected when it has a
`pull_request` trigger and the PR passes that trigger's filters: `branches`
and `branches-ignore` against BRANCH (the base), `paths` and `paths-ignore`
against the changed files. A trigger with no filter is always expected.
Prints the workflow's `name:` (its file name if it has none), one per line.
With --jobs, prints instead one `WORKFLOW<TAB>JOB` line for each job of each
expected workflow: the job's `name:`, else its id.

There is no YAML parser on every machine this runs on, so this reads the
`on:` block by indentation: block mappings, block lists and one-line
`[a, b]` lists, which is all these workflows use. An `on:` it cannot read
exits 2 rather than guessing.
"""
import glob
import os
import re
import sys


def unquote(v):
    v = v.strip()
    if len(v) >= 2 and v[0] == v[-1] and v[0] in "\"'":
        return v[1:-1]
    return v


def strip_comment(line):
    out, quote = [], None
    for i, ch in enumerate(line):
        if quote:
            if ch == quote:
                quote = None
        elif ch in "\"'":
            quote = ch
        elif ch == "#" and (i == 0 or line[i - 1] in " \t"):
            break
        out.append(ch)
    return "".join(out).rstrip()


def flow_list(v):
    inner = v.strip()[1:-1]
    return [unquote(x) for x in inner.split(",") if x.strip()]


def parse_on(lines):
    """Returns {trigger: {key: [values]}} from the top-level `on:` block."""
    rows = []  # (indent, text)
    for raw in lines:
        t = strip_comment(raw)
        if t.strip():
            rows.append((len(t) - len(t.lstrip()), t.strip()))
    start = None
    for i, (ind, t) in enumerate(rows):
        if ind == 0 and re.match(r"^(on|\"on\"|'on'):", t):
            start = i
            break
    if start is None:
        return None
    head = rows[start][1].split(":", 1)[1].strip()
    if head.startswith("["):
        return {k: {} for k in flow_list(head)}
    if head:
        return {unquote(head): {}}
    triggers, cur, key = {}, None, None
    for ind, t in rows[start + 1:]:
        if ind == 0:
            break
        if ind == 2 and not t.startswith("- "):
            name, _, rest = t.partition(":")
            cur, key = unquote(name), None
            triggers[cur] = {}
            if rest.strip():
                raise ValueError(t)
        elif ind == 4 and cur is not None and not t.startswith("- "):
            name, _, rest = t.partition(":")
            key = unquote(name)
            rest = rest.strip()
            triggers[cur][key] = flow_list(rest) if rest.startswith("[") else (
                [unquote(rest)] if rest else [])
        elif t.startswith("- ") and cur is not None and key is not None:
            triggers[cur][key].append(unquote(t[2:]))
    return triggers


def parse_jobs(lines):
    """Returns the name of each job in the top-level `jobs:` block, in order:
    its `name:` if it has one, else its id."""
    rows = []
    for raw in lines:
        t = strip_comment(raw)
        if t.strip():
            rows.append((len(t) - len(t.lstrip()), t.strip()))
    start = None
    for i, (ind, t) in enumerate(rows):
        if ind == 0 and t == "jobs:":
            start = i
            break
    if start is None:
        return []
    jobs, job_ind = [], None
    for ind, t in rows[start + 1:]:
        if ind == 0:
            break
        if job_ind is None:
            job_ind = ind
        if ind == job_ind:
            name, _, rest = t.partition(":")
            if rest.strip() or t.startswith("- "):
                raise ValueError(t)
            jobs.append(unquote(name))
        elif ind == job_ind + 2 and t.startswith("name:"):
            jobs[-1] = unquote(t.split(":", 1)[1])
    return jobs


def glob_re(pat):
    out, i = [], 0
    while i < len(pat):
        c = pat[i]
        if pat[i:i + 3] == "**/":
            out.append("(?:.*/)?")
            i += 3
            continue
        if pat[i:i + 2] == "**":
            out.append(".*")
            i += 2
            continue
        out.append({"*": "[^/]*", "?": "[^/]"}.get(c, re.escape(c)))
        i += 1
    return re.compile("^" + "".join(out) + "$")


def matches(patterns, value):
    """GitHub filter semantics: patterns apply in order, `!` removes."""
    hit = False
    for p in patterns:
        if p.startswith("!"):
            if glob_re(p[1:]).match(value):
                hit = False
        elif glob_re(p).match(value):
            hit = True
    return hit


def expected(flt, base, files):
    if flt.get("branches") and not matches(flt["branches"], base):
        return False
    if flt.get("branches-ignore") and matches(flt["branches-ignore"], base):
        return False
    if flt.get("paths"):
        return any(matches(flt["paths"], f) for f in files)
    if flt.get("paths-ignore"):
        return any(not matches(flt["paths-ignore"], f) for f in files)
    return True


def main(argv):
    base, d, want_jobs = None, ".github/workflows", False
    args = list(argv)
    while args:
        a = args.pop(0)
        if a == "--base":
            base = args.pop(0)
        elif a == "--dir":
            d = args.pop(0)
        elif a == "--jobs":
            want_jobs = True
        else:
            sys.exit("usage: expected.py [--jobs] --base BRANCH [--dir DIR] < files")
    if base is None:
        sys.exit("usage: expected.py [--jobs] --base BRANCH [--dir DIR] < files")
    files = [line.strip() for line in sys.stdin if line.strip()]
    paths = sorted(glob.glob(os.path.join(d, "*.yml")) + glob.glob(os.path.join(d, "*.yaml")))
    for path in paths:
        with open(path) as fh:
            lines = fh.read().splitlines()
        try:
            triggers = parse_on(lines)
        except ValueError as e:
            print("expected.py: cannot read the on: block of %s (%s)" % (path, e), file=sys.stderr)
            return 2
        if triggers is None or "pull_request" not in triggers:
            continue
        if not expected(triggers["pull_request"], base, files):
            continue
        name = os.path.basename(path)
        for line in lines:
            m = re.match(r"^name:\s*(.*)$", line)
            if m:
                name = unquote(strip_comment(m.group(1))) or name
                break
        if not want_jobs:
            print(name)
            continue
        try:
            jobs = parse_jobs(lines)
        except ValueError as e:
            print("expected.py: cannot read the jobs: block of %s (%s)" % (path, e), file=sys.stderr)
            return 2
        for job in jobs:
            print("%s\t%s" % (name, job))
    return 0


sys.exit(main(sys.argv[1:]))
