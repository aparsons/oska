#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Write the loopback test server's files (crates/oskac/tests/tls/pki/).

    python3 -I scripts/tls-fixtures/generate.py crates/oskac/tests/tls/pki

Each `chain_*.hex` is the server's certificate chain, one certificate a line, as
hexadecimal DER, leaf first; `root.hex` is the root the clients trust;
`leaf.key.hex` is the server's signing key (a P-256 scalar, shared by every leaf),
and `leaf.pem` and `chain_good.pem` are the same for tools such as openssl.

`leaf.key.pem` and `leaf.key.hex` are TEST-ONLY keys. They are committed to the
repository, so they are public; the test CA that signs the leaves is trusted by
nothing but the tests, which add it as a root by hand. Never use them, or a key made
by this script, anywhere else.
"""

import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pki  # noqa: E402
from cryptography.hazmat.primitives import serialization  # noqa: E402


def hex_of(cert):
    return cert.public_bytes(pki.DER).hex()


def main():
    out = sys.argv[1]
    os.makedirs(out, exist_ok=True)
    made = pki.build()

    def write(name, text):
        with open(os.path.join(out, name), "w") as f:
            f.write(text)

    write("root.hex", hex_of(made["ca"]) + "\n")
    write("rogue_root.hex", hex_of(made["rogue"]) + "\n")
    write("leaf.key.hex", pki.scalar_hex(made["leaf_key"]) + "\n")
    chains = {
        "good": [made["good"], made["intermediate"]],
        "wrong_host": [made["wrong_host"], made["intermediate"]],
        "expired": [made["expired"], made["intermediate"]],
        "unknown": [made["unknown"], made["rogue"]],
        "no_intermediate": [made["good"]],
    }
    for label, chain in chains.items():
        write("chain_%s.hex" % label, "".join(hex_of(c) + "\n" for c in chain))
    write(
        "chain_good.pem",
        "".join(c.public_bytes(serialization.Encoding.PEM).decode() for c in chains["good"]),
    )
    write(
        "leaf.key.pem",
        made["leaf_key"].private_bytes(
            serialization.Encoding.PEM,
            serialization.PrivateFormat.PKCS8,
            serialization.NoEncryption(),
        ).decode(),
    )
    write("root.pem", made["ca"].public_bytes(serialization.Encoding.PEM).decode())


main()
