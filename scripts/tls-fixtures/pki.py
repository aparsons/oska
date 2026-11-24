# SPDX-License-Identifier: Apache-2.0
"""The test PKI of the TLS tests (#244): a CA, an intermediate and server certificates.

Used by generate.py (the loopback test server's files) and record.py (the
recorded handshakes). Needs the `cryptography` package, version 43 or later.
Every certificate is valid from 1970-01-01 to 2099-12-31, so it verifies at the
test host's clock (the Unix epoch) and on a real clock; the expired one ends in
2000. The keys come from fixed seeds, so a run gives the same keys; the
signatures of `cryptography` are randomized, so the certificates differ between
runs and the committed files are one run.
"""

import datetime
import ipaddress

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, rsa
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

DER = serialization.Encoding.DER
UTC = datetime.timezone.utc
START = datetime.datetime(1970, 1, 1, tzinfo=UTC)
END = datetime.datetime(2099, 12, 31, tzinfo=UTC)
EXPIRED_END = datetime.datetime(2000, 1, 1, tzinfo=UTC)


def name(common_name):
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, common_name)])


def ec_key(seed, curve=ec.SECP256R1):
    """A key from a fixed seed, so the files do not change from run to run."""
    return ec.derive_private_key(seed, curve())


def scalar_hex(key):
    return "%064x" % key.private_numbers().private_value


def certificate(subject, key, issuer_name, issuer_key, serial, ca=False, sans=None, end=END, path_len=None):
    builder = (
        x509.CertificateBuilder()
        .subject_name(subject)
        .issuer_name(issuer_name)
        .public_key(key.public_key())
        .serial_number(serial)
        .not_valid_before(START)
        .not_valid_after(end)
    )
    if ca:
        builder = builder.add_extension(x509.BasicConstraints(ca=True, path_length=path_len), critical=True)
        builder = builder.add_extension(
            x509.KeyUsage(
                digital_signature=False, content_commitment=False, key_encipherment=False,
                data_encipherment=False, key_agreement=False, key_cert_sign=True, crl_sign=False,
                encipher_only=False, decipher_only=False,
            ),
            critical=True,
        )
    else:
        builder = builder.add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
        builder = builder.add_extension(
            x509.KeyUsage(
                digital_signature=True, content_commitment=False, key_encipherment=False,
                data_encipherment=False, key_agreement=False, key_cert_sign=False, crl_sign=False,
                encipher_only=False, decipher_only=False,
            ),
            critical=True,
        )
        builder = builder.add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
        names = []
        for san in sans or []:
            try:
                names.append(x509.IPAddress(ipaddress.ip_address(san)))
            except ValueError:
                names.append(x509.DNSName(san))
        builder = builder.add_extension(x509.SubjectAlternativeName(names), critical=False)
    builder = builder.add_extension(x509.SubjectKeyIdentifier.from_public_key(key.public_key()), critical=False)
    if issuer_key is not key:
        builder = builder.add_extension(
            x509.AuthorityKeyIdentifier.from_issuer_public_key(issuer_key.public_key()), critical=False
        )
    algorithm = hashes.SHA384() if getattr(issuer_key, "curve", None) is not None and issuer_key.curve.name == "secp384r1" else hashes.SHA256()
    return builder.sign(issuer_key, algorithm)


def build():
    """Every key and certificate of the loopback tests, by name."""
    ca_key = ec_key(1001)
    intermediate_key = ec_key(1002)
    leaf_key = ec_key(1003)
    rogue_key = ec_key(1004)
    ca = certificate(name("Oska Test Root"), ca_key, name("Oska Test Root"), ca_key, 1, ca=True)
    intermediate = certificate(name("Oska Test Intermediate"), intermediate_key, ca.subject, ca_key, 2, ca=True, path_len=0)
    rogue = certificate(name("Oska Rogue Root"), rogue_key, name("Oska Rogue Root"), rogue_key, 3, ca=True)
    hosts = ["localhost", "127.0.0.1", "::1"]
    good = certificate(name("localhost"), leaf_key, intermediate.subject, intermediate_key, 10, sans=hosts)
    wrong_host = certificate(name("other.test"), leaf_key, intermediate.subject, intermediate_key, 11, sans=["other.test"])
    expired = certificate(name("localhost"), leaf_key, intermediate.subject, intermediate_key, 12, sans=hosts, end=EXPIRED_END)
    unknown = certificate(name("localhost"), leaf_key, rogue.subject, rogue_key, 13, sans=hosts)
    # Leaves with other kinds of key, for the recorded handshakes (record.py).
    rsa_key = rsa.generate_private_key(65537, 2048)
    p384_key = ec_key(1005, ec.SECP384R1)
    rsa_leaf = certificate(name("localhost"), rsa_key, intermediate.subject, intermediate_key, 14, sans=hosts)
    p384_leaf = certificate(name("localhost"), p384_key, intermediate.subject, intermediate_key, 15, sans=hosts)
    return {
        "rsa_key": rsa_key,
        "rsa_leaf": rsa_leaf,
        "p384_key": p384_key,
        "p384_leaf": p384_leaf,
        "ca": ca,
        "intermediate": intermediate,
        "rogue": rogue,
        "good": good,
        "wrong_host": wrong_host,
        "expired": expired,
        "unknown": unknown,
        "leaf_key": leaf_key,
    }
