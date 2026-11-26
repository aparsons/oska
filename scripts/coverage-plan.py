#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Print the Coverage workflow's job matrix, as JSON.

  python3 -I scripts/coverage-plan.py

Instrumented, the linux shards of scripts/test-shard.sh take longer than the
60 minutes a job may run, so coverage cuts the same tests finer: the `site`
shard whole, each heavy test file of `native` as a job of its own (the
corpus test in three slices, as the macOS shards do), and the `rest` shard
dealt round-robin into REST_JOBS jobs. Every test still runs in exactly one
job. Each entry is {"name", "lines"}: `lines` is a plan for
scripts/test-shard.sh (OSKA_SHARD_LINES).
"""

import json
import subprocess

REST_JOBS = 6


def plan(shard):
    out = subprocess.run(
        ["scripts/test-shard.sh", "linux", shard, "--plan"],
        check=True, capture_output=True, text=True,
    ).stdout
    return [line for line in out.splitlines() if line.strip()]


jobs = [{"name": "site", "lines": "\n".join(plan("site"))}]
for line in plan("native"):
    if line == "oskac test native_tests":
        for k in range(3):
            jobs.append({"name": f"native_tests-{k}", "lines": f"oskac test native_tests/{k}"})
    else:
        jobs.append({"name": line.split()[-1], "lines": line})
rest = plan("rest")
for k in range(REST_JOBS):
    jobs.append({"name": f"rest-{k + 1}", "lines": "\n".join(rest[k::REST_JOBS])})
print(json.dumps(jobs))
