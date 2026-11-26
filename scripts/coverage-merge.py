#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Merge lcov files, and write the coverage report tables.

  python3 -I scripts/coverage-merge.py OUT_DIR LCOV...

Reads each lcov file (one per test shard), sums the hit counts of every
source line, and writes OUT_DIR/lcov.info (the merged file, paths relative
to the repository root) and OUT_DIR/summary.md: a table with a row a crate,
then one with a row a file. A line is covered when any shard ran it.
"""

import os
import sys
from collections import defaultdict


def read(path, root, hits):
    source = None
    with open(path, encoding="utf-8") as f:
        for line in f:
            line = line.rstrip("\n")
            if line.startswith("SF:"):
                source = line[3:]
                if source.startswith(root):
                    source = source[len(root):]
            elif line.startswith("DA:") and source is not None:
                number, count = line[3:].split(",")[:2]
                hits[source][int(number)] += int(count)


def main():
    if len(sys.argv) < 3:
        sys.exit("usage: coverage-merge.py OUT_DIR LCOV...")
    out = sys.argv[1]
    root = os.getcwd().rstrip("/") + "/"
    hits = defaultdict(lambda: defaultdict(int))
    for path in sys.argv[2:]:
        read(path, root, hits)
    os.makedirs(out, exist_ok=True)

    with open(os.path.join(out, "lcov.info"), "w", encoding="utf-8") as f:
        for source in sorted(hits):
            f.write(f"SF:{source}\n")
            for number in sorted(hits[source]):
                f.write(f"DA:{number},{hits[source][number]}\n")
            lines = hits[source]
            f.write(f"LF:{len(lines)}\nLH:{sum(1 for c in lines.values() if c > 0)}\nend_of_record\n")

    crates = defaultdict(lambda: [0, 0])
    files = []
    for source, lines in hits.items():
        found = len(lines)
        hit = sum(1 for c in lines.values() if c > 0)
        parts = source.split("/")
        crate = parts[1] if parts[0] == "crates" and len(parts) > 1 else parts[0]
        crates[crate][0] += found
        crates[crate][1] += hit
        files.append((source, found, hit))

    def pct(hit, found):
        return f"{100 * hit / found:.1f}%" if found else "n/a"

    total_found = sum(c[0] for c in crates.values())
    total_hit = sum(c[1] for c in crates.values())
    md = ["| Crate | Lines | Covered | Coverage |", "| --- | ---: | ---: | ---: |"]
    for crate in sorted(crates):
        found, hit = crates[crate]
        md.append(f"| {crate} | {found} | {hit} | {pct(hit, found)} |")
    md.append(f"| **Total** | {total_found} | {total_hit} | **{pct(total_hit, total_found)}** |")
    md += ["", "## By file", "", "| File | Lines | Covered | Coverage |", "| --- | ---: | ---: | ---: |"]
    for source, found, hit in sorted(files):
        md.append(f"| `{source}` | {found} | {hit} | {pct(hit, found)} |")
    with open(os.path.join(out, "summary.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(md) + "\n")


main()
