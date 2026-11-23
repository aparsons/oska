#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Record TLS 1.3 handshakes of std.tls's engine against openssl's server (#244).

    python3 -I scripts/tls-fixtures/record.py <oskac> crates/oskac/tests/tls/recorded

For each profile it starts `openssl s_server`, runs `record.osk` (the engine over
a real connection, with fixed randomness and a fixed time) and writes
`<profile>.txt`: a few header lines (`name`, `time`, `random`, `root`), then one
line a TLS record, `C <hex>` for what the engine sent and `S <hex>` for what the
server sent, in order. `replay.osk` plays a file back offline through the engine,
so the transcripts never expire: the certificates are valid from 1970 to 2099 and
the time is stored beside them. openssl is an independent implementation, so a
recording is evidence that the engine interoperates with another stack.
Needs openssl 3 and the `cryptography` package.
"""

import os
import socket
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pki  # noqa: E402
from cryptography.hazmat.primitives import serialization  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
RECORD = os.path.join(HERE, "../../crates/oskac/tests/tls/record.osk")
RANDOM = "".join("%02x" % ((i * 7 + 3) % 256) for i in range(64))
TIME = 1750000000  # 2025-06-15


def pem(obj):
    if hasattr(obj, "public_bytes"):
        return obj.public_bytes(serialization.Encoding.PEM)
    return obj.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()
    )


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def split_records(data):
    """The TLS records in `data`, each as bytes."""
    records = []
    at = 0
    while at + 5 <= len(data):
        size = int.from_bytes(data[at + 3 : at + 5], "big")
        records.append(data[at : at + 5 + size])
        at += 5 + size
    assert at == len(data), "a record is cut off"
    return records


def record(oskac, outdir, made, profile, leaf, key, options):
    work = tempfile.mkdtemp()
    for name, obj in (("leaf.pem", leaf), ("key.pem", key), ("chain.pem", made["intermediate"])):
        with open(os.path.join(work, name), "wb") as f:
            f.write(pem(obj))
    port = free_port()
    server = subprocess.Popen(
        ["openssl", "s_server", "-tls1_3", "-accept", str(port), "-cert", os.path.join(work, "leaf.pem"),
         "-key", os.path.join(work, "key.pem"), "-cert_chain", os.path.join(work, "chain.pem"),
         "-www", "-quiet"] + options,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        time.sleep(1)
        root = made["ca"].public_bytes(pki.DER).hex()
        out = subprocess.run(
            [oskac, "run", RECORD, "--", str(port), "localhost", RANDOM, str(TIME), root],
            check=True, capture_output=True, text=True,
        ).stdout
    finally:
        server.kill()
        server.wait()
    lines = [l for l in out.splitlines() if l]
    assert all(l[0] in "CS" for l in lines), out
    body = []
    for line in lines:
        side, data = line.split(" ")
        if side == "C":
            body.append(line)
        else:
            for r in split_records(bytes.fromhex(data)):
                body.append("S " + r.hex())
    header = ["name localhost", "time %d" % TIME, "random " + RANDOM, "root " + root]
    with open(os.path.join(outdir, profile + ".txt"), "w") as f:
        f.write("\n".join(header + body) + "\n")
    print(profile, len(body), "lines")


def main():
    oskac, outdir = sys.argv[1], sys.argv[2]
    os.makedirs(outdir, exist_ok=True)
    made = pki.build()
    ec_leaf, ec_key = made["good"], made["leaf_key"]
    record(oskac, outdir, made, "chacha_x25519_ecdsa_p256", ec_leaf, ec_key, [])
    record(oskac, outdir, made, "aes_x25519_ecdsa_p256", ec_leaf, ec_key,
           ["-ciphersuites", "TLS_AES_128_GCM_SHA256"])
    record(oskac, outdir, made, "hello_retry_request_p256", ec_leaf, ec_key, ["-groups", "P-256"])
    record(oskac, outdir, made, "rsa_pss", made["rsa_leaf"], made["rsa_key"], [])
    record(oskac, outdir, made, "ecdsa_p384", made["p384_leaf"], made["p384_key"], [])


main()
