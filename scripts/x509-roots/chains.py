#!/usr/bin/env python3
"""Generate the crafted-chain test files for std.x509 (crates/oskac/tests/vectors/x509_chains.osk, fixtures/x509_cases.osk).

    python3 -I scripts/x509-roots/chains.py full > crates/oskac/tests/vectors/x509_chains.osk
    python3 -I scripts/x509-roots/chains.py quick > crates/oskac/tests/fixtures/x509_cases.osk

`full` is every chain, which checks about forty signatures and so runs natively only
(`native_x509.rs`): the interpreter takes over a minute on it. `quick` is the cases that
need no signature check but one chain, for the corpus that runs both backends.

Needs the `cryptography` package (version 41 or later). Keys and the random
signatures are new on every run, so a run gives new bytes and the same tests:
the committed file is one run of it. Every certificate is valid from
2024-01-01 to 2034-01-01 unless a case says otherwise, and the tests check
them at NOW, 2025-06-15.
"""

import datetime
import re
import ipaddress
import sys

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, ed25519, padding, rsa
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

NOW = int(datetime.datetime(2025, 6, 15, tzinfo=datetime.timezone.utc).timestamp())
START = datetime.datetime(2024, 1, 1)
END = datetime.datetime(2034, 1, 1)
DER = serialization.Encoding.DER
mode = sys.argv[1] if len(sys.argv) > 1 else "full"
assert mode in ("full", "quick"), "give `full` or `quick`"

SERVER = [ExtendedKeyUsageOID.SERVER_AUTH]

certs = {}  # name -> DER bytes, in the order made


def name(cn):
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])


def ec_key(curve=ec.SECP256R1):
    return ec.generate_private_key(curve())


def rsa_key(bits=2048):
    return rsa.generate_private_key(65537, bits)


def make(
    label,
    subject,
    key,
    issuer=None,
    issuer_key=None,
    ca=None,
    path_len=None,
    key_usage=None,
    eku="auto",
    ski=None,
    aki=None,
    san=None,
    not_before=START,
    not_after=END,
    hash_alg=hashes.SHA256,
    pss=False,
    extra=(),
    name_constraints=None,
    sign_hash="default",
):
    """Make a certificate for `key`, signed by `issuer_key` (itself if omitted)."""
    issuer_key = issuer_key or key
    builder = (
        x509.CertificateBuilder()
        .subject_name(name(subject))
        .issuer_name(name(issuer or subject))
        .public_key(key.public_key())
        .serial_number(x509.random_serial_number())
        .not_valid_before(not_before)
        .not_valid_after(not_after)
    )
    if ca is not None:
        builder = builder.add_extension(x509.BasicConstraints(ca=ca, path_length=path_len), True)
    if key_usage is not None:
        flags = dict(
            digital_signature=False, content_commitment=False, key_encipherment=False,
            data_encipherment=False, key_agreement=False, key_cert_sign=False,
            crl_sign=False, encipher_only=False, decipher_only=False,
        )
        flags.update({k: True for k in key_usage})
        builder = builder.add_extension(x509.KeyUsage(**flags), True)
    # A server certificate names server authentication: `auto` gives every certificate that is not a CA that,
    # and `False` leaves the extension out.
    if eku == "auto":
        eku = None if ca else SERVER
    if eku:
        builder = builder.add_extension(x509.ExtendedKeyUsage(eku), False)
    if ski is not None:
        builder = builder.add_extension(x509.SubjectKeyIdentifier(ski), False)
    if aki is not None:
        builder = builder.add_extension(
            x509.AuthorityKeyIdentifier(key_identifier=aki, authority_cert_issuer=None, authority_cert_serial_number=None),
            False,
        )
    if san is not None:
        builder = builder.add_extension(x509.SubjectAlternativeName(san), False)
    if name_constraints is not None:
        builder = builder.add_extension(name_constraints, True)
    for ext, critical in extra:
        builder = builder.add_extension(ext, critical)
    if isinstance(issuer_key, ed25519.Ed25519PrivateKey):
        cert = builder.sign(issuer_key, None)
    elif pss:
        cert = builder.sign(
            issuer_key, hash_alg(), rsa_padding=padding.PSS(padding.MGF1(hash_alg()), hash_alg().digest_size)
        )
    else:
        cert = builder.sign(issuer_key, hash_alg())
    certs[label] = cert.public_bytes(DER)
    return cert


def dns(*names):
    return [x509.DNSName(n) for n in names]


CA_KU = ["key_cert_sign", "crl_sign"]
LEAF_KU = ["digital_signature"]

# ---- The EC hierarchy -----------------------------------------------------------------

root_a_key = ec_key()
root_a = make("root_a", "Test EC Root", root_a_key, ca=True, key_usage=CA_KU)
int_a_key = ec_key(ec.SECP384R1)
int_a = make(
    "int_a", "Test EC Intermediate", int_a_key, issuer="Test EC Root", issuer_key=root_a_key,
    ca=True, path_len=0, key_usage=CA_KU,
)
leaf_a_key = ec_key()
SAN_MAIN = dns("example.test", "WWW.Example.Test", "*.wild.test", "*.test", "w*.example.test", "*.sub.example.test") + [
    x509.IPAddress(ipaddress.ip_address("192.0.2.7")),
    x509.IPAddress(ipaddress.ip_address("2001:db8::1")),
]
make(
    "leaf_a", "leaf.example.test", leaf_a_key, issuer="Test EC Intermediate", issuer_key=int_a_key,
    key_usage=LEAF_KU, eku=SERVER, san=SAN_MAIN, hash_alg=hashes.SHA384,
    extra=[(x509.UnrecognizedExtension(x509.ObjectIdentifier("1.3.6.1.4.1.99999.1"), b"\x05\x00"), False)],
)
make(
    "leaf_a_sha512", "leaf.example.test", ec_key(), issuer="Test EC Intermediate", issuer_key=int_a_key,
    key_usage=LEAF_KU, eku=[ExtendedKeyUsageOID.ANY_EXTENDED_KEY_USAGE], san=dns("sha512.test"), hash_alg=hashes.SHA512,
)
make(
    "leaf_no_eku", "noeku.test", ec_key(), issuer="Test EC Intermediate", issuer_key=int_a_key, key_usage=LEAF_KU,
    san=dns("example.test"), eku=False,
)
make("leaf_a_nosan", "nosan.test", ec_key(), issuer="Test EC Intermediate", issuer_key=int_a_key, key_usage=LEAF_KU)

# ---- The RSA hierarchy ----------------------------------------------------------------

root_b_key = rsa_key()
make("root_b", "Test RSA Root", root_b_key, ca=True, key_usage=CA_KU)
int_b_key = rsa_key()
make(
    "int_b", "Test RSA Intermediate", int_b_key, issuer="Test RSA Root", issuer_key=root_b_key,
    ca=True, key_usage=CA_KU,
)
for label, hash_alg, pss in [
    ("leaf_b_sha256", hashes.SHA256, False),
    ("leaf_b_sha384", hashes.SHA384, False),
    ("leaf_b_sha512", hashes.SHA512, False),
    ("leaf_b_pss256", hashes.SHA256, True),
    ("leaf_b_pss384", hashes.SHA384, True),
]:
    make(
        label, "rsa.example.test", rsa_key(), issuer="Test RSA Intermediate", issuer_key=int_b_key,
        key_usage=["digital_signature", "key_encipherment"], eku=SERVER, san=dns("rsa.example.test"),
        hash_alg=hash_alg, pss=pss,
    )
make(
    "int_b_pss", "Test RSA PSS Intermediate", rsa_key(), issuer="Test RSA Root", issuer_key=root_b_key,
    ca=True, hash_alg=hashes.SHA256, pss=True,
)
# An intermediate signed with SHA-1 (not supported), and one with a 1024-bit key.
# (The library will not sign with SHA-1, so these are made with SHA-256 and the algorithm
# identifier, the same length, is changed to sha1WithRSAEncryption in both places: the
# signature then does not verify, but the algorithm is refused first.)
def as_sha1(label):
    old = bytes.fromhex("06092a864886f70d01010b0500")
    assert certs[label].count(old) == 2
    certs[label] = certs[label].replace(old, bytes.fromhex("06092a864886f70d0101050500"))


make(
    "int_b_sha1", "Test RSA SHA-1 Intermediate", int_b_key, issuer="Test RSA Root", issuer_key=root_b_key,
    ca=True,
)
as_sha1("int_b_sha1")
make(
    "leaf_b_under_sha1", "rsa.example.test", rsa_key(), issuer="Test RSA SHA-1 Intermediate", issuer_key=int_b_key,
    san=dns("rsa.example.test"),
)
make(
    "leaf_b_sha1", "rsa.example.test", rsa_key(), issuer="Test RSA Intermediate", issuer_key=int_b_key,
    san=dns("rsa.example.test"),
)
as_sha1("leaf_b_sha1")
small_key = rsa_key(1024)
make(
    "int_small", "Test Small Key Intermediate", small_key, issuer="Test RSA Root", issuer_key=root_b_key, ca=True,
)
make(
    "leaf_under_small", "small.example.test", ec_key(), issuer="Test Small Key Intermediate", issuer_key=small_key,
    san=dns("small.example.test"),
)

# ---- Ed25519 --------------------------------------------------------------------------

root_c_key = ed25519.Ed25519PrivateKey.generate()
make("root_c", "Test Ed25519 Root", root_c_key, ca=True, key_usage=CA_KU)
make(
    "leaf_c", "ed.example.test", ed25519.Ed25519PrivateKey.generate(), issuer="Test Ed25519 Root", issuer_key=root_c_key,
    key_usage=LEAF_KU, san=dns("ed.example.test"),
)

# ---- Single defects, under the EC hierarchy -------------------------------------------

def under_a(label, subject, **kw):
    kw.setdefault("san", dns("example.test"))
    kw.setdefault("key_usage", LEAF_KU)
    return make(label, subject, kw.pop("key", None) or ec_key(), issuer="Test EC Intermediate", issuer_key=int_a_key, **kw)


under_a("leaf_expired", "expired", not_before=datetime.datetime(2023, 1, 1), not_after=datetime.datetime(2025, 1, 1))
under_a("leaf_future", "future", not_before=datetime.datetime(2026, 1, 1), not_after=datetime.datetime(2027, 1, 1))
under_a("leaf_eku_client", "client", eku=[ExtendedKeyUsageOID.CLIENT_AUTH])
under_a("leaf_ku_enc", "kuenc", key_usage=["key_encipherment"])
under_a("leaf_unknown_critical", "critical", extra=[
    (x509.UnrecognizedExtension(x509.ObjectIdentifier("1.3.6.1.4.1.99999.2"), b"\x05\x00"), True)
])
under_a("leaf_distrust", "distrust", not_before=datetime.datetime(2025, 3, 1), not_after=datetime.datetime(2026, 3, 1))

# An intermediate that is not a CA, in three ways, each with a leaf.
for label, kw in [
    ("noca", dict()),
    ("caflag", dict(ca=False)),
    ("ku", dict(ca=True, key_usage=["digital_signature"])),
    ("eku", dict(ca=True, key_usage=CA_KU, eku=[ExtendedKeyUsageOID.CLIENT_AUTH])),
]:
    k = ec_key()
    make(f"int_{label}", f"Bad {label}", k, issuer="Test EC Root", issuer_key=root_a_key, **kw)
    make(
        f"leaf_under_{label}", "x.example.test", ec_key(), issuer=f"Bad {label}", issuer_key=k,
        san=dns("example.test"), key_usage=LEAF_KU,
    )

# An expired intermediate.
k = ec_key()
make(
    "int_expired", "Expired Intermediate", k, issuer="Test EC Root", issuer_key=root_a_key, ca=True, key_usage=CA_KU,
    not_before=datetime.datetime(2020, 1, 1), not_after=datetime.datetime(2025, 1, 1),
)
make("leaf_under_expired", "x", ec_key(), issuer="Expired Intermediate", issuer_key=k, san=dns("example.test"))

# Path length: root -> int0 (pathLen 0) -> int1 -> leaf.
k0, k1 = ec_key(), ec_key()
make("int_p0", "Path 0", k0, issuer="Test EC Root", issuer_key=root_a_key, ca=True, path_len=0, key_usage=CA_KU)
make("int_p1", "Path 1", k1, issuer="Path 0", issuer_key=k0, ca=True, key_usage=CA_KU)
make("leaf_p1", "x", ec_key(), issuer="Path 1", issuer_key=k1, san=dns("example.test"))
# And the same with pathLen 1, which allows it.
k2, k3 = ec_key(), ec_key()
make("int_q0", "Path 1 Allowed", k2, issuer="Test EC Root", issuer_key=root_a_key, ca=True, path_len=1, key_usage=CA_KU)
make("int_q1", "Path Below", k3, issuer="Path 1 Allowed", issuer_key=k2, ca=True, key_usage=CA_KU)
make("leaf_q1", "x", ec_key(), issuer="Path Below", issuer_key=k3, san=dns("example.test"))

# Name constraints.
nc_key = ec_key()
make(
    "int_nc", "Constrained", nc_key, issuer="Test EC Root", issuer_key=root_a_key, ca=True, key_usage=CA_KU,
    name_constraints=x509.NameConstraints(
        permitted_subtrees=[x509.DNSName("permitted.test"), x509.IPAddress(ipaddress.ip_network("192.0.2.0/24"))],
        excluded_subtrees=[x509.DNSName("bad.permitted.test")],
    ),
)
for label, sans in [
    ("nc_ok", dns("permitted.test", "a.b.permitted.test", "*.permitted.test")),
    ("nc_other", dns("other.test")),
    ("nc_mixed", dns("permitted.test", "other.test")),
    ("nc_excluded", dns("x.bad.permitted.test")),
    ("nc_wild_other", dns("*.other.test")),
    ("nc_ip_ok", [x509.IPAddress(ipaddress.ip_address("192.0.2.5"))]),
    ("nc_ip_out", [x509.IPAddress(ipaddress.ip_address("198.51.100.1"))]),
]:
    make(f"leaf_{label}", "x", ec_key(), issuer="Constrained", issuer_key=nc_key, san=sans, key_usage=LEAF_KU)
nc2_key = ec_key()
make(
    "int_nc_dot", "Constrained Dot", nc2_key, issuer="Test EC Root", issuer_key=root_a_key, ca=True, key_usage=CA_KU,
    name_constraints=x509.NameConstraints(permitted_subtrees=[x509.DNSName(".sub.test")], excluded_subtrees=None),
)
make("leaf_dot_ok", "x", ec_key(), issuer="Constrained Dot", issuer_key=nc2_key, san=dns("a.sub.test"), key_usage=LEAF_KU)
make("leaf_dot_bare", "x", ec_key(), issuer="Constrained Dot", issuer_key=nc2_key, san=dns("sub.test"), key_usage=LEAF_KU)

# A second intermediate with the name of int_a and another key (a cross-sign or a rollover).
make(
    "int_a_other_key", "Test EC Intermediate", ec_key(ec.SECP384R1), issuer="Test EC Root", issuer_key=root_a_key,
    ca=True, path_len=0, key_usage=CA_KU,
)

# A chain to a root that no store has.
root_x_key = ec_key()
make("root_x", "Unknown Root", root_x_key, ca=True, key_usage=CA_KU)
make("leaf_x", "x", ec_key(), issuer="Unknown Root", issuer_key=root_x_key, san=dns("example.test"), key_usage=LEAF_KU)

# A self-signed server certificate.
self_key = ec_key()
make("self_signed", "self.test", self_key, san=dns("self.test"), key_usage=LEAF_KU)

# Name constraints apply to every certificate below the constraining one: a CA under `Constrained` that names
# a host outside it (leaf_under_sub_bad), and one that does not (leaf_under_sub_ok).
for label, san in [("bad", "evil.other.test"), ("ok", "ok.permitted.test")]:
    k = ec_key()
    make(
        f"sub_{label}", f"Sub {label}", k, issuer="Constrained", issuer_key=nc_key, ca=True, key_usage=CA_KU,
        san=dns(san),
    )
    make(
        f"leaf_under_sub_{label}", "x", ec_key(), issuer=f"Sub {label}", issuer_key=k, san=dns("a.permitted.test"),
        key_usage=LEAF_KU,
    )

# Depth: a chain of eight intermediates under the root, and the seven that fit.
parent, parent_key = "Test EC Root", root_a_key
for n in range(1, 9):
    k = ec_key()
    make(f"deep_{n}", f"Deep {n}", k, issuer=parent, issuer_key=parent_key, ca=True, key_usage=CA_KU)
    if n == 7:
        make("leaf_deep7", "x", ec_key(), issuer=f"Deep {n}", issuer_key=k, san=dns("example.test"), key_usage=LEAF_KU)
    parent, parent_key = f"Deep {n}", k
make("leaf_deep8", "x", ec_key(), issuer=parent, issuer_key=parent_key, san=dns("example.test"), key_usage=LEAF_KU)

# Key identifiers: two intermediates with one name, and a leaf that names the first one's key.
good_key = ec_key(ec.SECP384R1)
make(
    "int_k_good", "Keyed Intermediate", good_key, issuer="Test EC Root", issuer_key=root_a_key, ca=True,
    key_usage=CA_KU, ski=b"good-key-id-0001",
)
make(
    "int_k_decoy", "Keyed Intermediate", ec_key(ec.SECP384R1), issuer="Test EC Root", issuer_key=root_a_key, ca=True,
    key_usage=CA_KU, ski=b"decoy-key-id-002",
)
make(
    "leaf_k", "x", ec_key(), issuer="Keyed Intermediate", issuer_key=good_key, san=dns("example.test"),
    key_usage=LEAF_KU, aki=b"good-key-id-0001",
)
# A root with a key identifier, and leaves that name it rightly and wrongly.
root_k_key = ec_key()
make("root_k", "Keyed Root", root_k_key, ca=True, key_usage=CA_KU, ski=b"root-key-id-0001")
make(
    "leaf_root_k", "x", ec_key(), issuer="Keyed Root", issuer_key=root_k_key, san=dns("example.test"),
    key_usage=LEAF_KU, aki=b"root-key-id-0001",
)
make(
    "leaf_root_k_wrong", "x", ec_key(), issuer="Keyed Root", issuer_key=root_k_key, san=dns("example.test"),
    key_usage=LEAF_KU, aki=b"some-other-key-0",
)

# ---- Malformed encodings, made by hand from leaf_a --------------------------------------

good = certs["leaf_a"]


def flip(data, index):
    out = bytearray(data)
    out[index] ^= 0x01
    return bytes(out)


mal = {}
mal["trailing"] = good + b"\x00"
mal["truncated"] = good[:-1]
mal["empty"] = b""
# The outer length in the long form with a leading zero (not minimal): 30 82 LL LL -> 30 83 00 LL LL.
assert good[0] == 0x30 and good[1] == 0x82
mal["long_length"] = b"\x30\x83\x00" + good[2:4] + good[4:]
# The outer length in the long form where the short form would do for the inner TLV: 30 82 -> replace
# the first inner SEQUENCE's header 30 82 LL LL by 30 83 00 LL LL and fix the outer length.
inner_len = int.from_bytes(good[6:8], "big")
assert good[4] == 0x30 and good[5] == 0x82
body = b"\x30\x83\x00" + good[6:8] + good[8 : 8 + inner_len] + good[8 + inner_len :]
mal["inner_long_length"] = b"\x30\x82" + len(body).to_bytes(2, "big") + body
# An indefinite length.
mal["indefinite"] = b"\x30\x80" + good[4:] + b"\x00\x00"
# A BER boolean (critical = 01 01 01) is not DER: find the first `01 01 ff` and change ff to 01.
i = good.index(b"\x01\x01\xff")
mal["ber_boolean"] = good[: i + 2] + b"\x01" + good[i + 3 :]
# Not a SEQUENCE.
mal["not_sequence"] = b"\x31" + good[1:]
# A high tag number form.
mal["high_tag"] = b"\x3f\x00" + good
# A version of 2 + 3 = v4: [0] A0 03 02 01 02 -> 03.
j = good.index(b"\xa0\x03\x02\x01\x02")
mal["version_4"] = good[: j + 4] + b"\x03" + good[j + 5 :]
# A time with a non-digit: the first UTCTime 17 0d "240101000000Z".
t = good.index(b"\x17\x0d")
mal["bad_time"] = good[: t + 2] + b"24x101000000Z" + good[t + 15 :]
# Month 13.
mal["month_13"] = good[: t + 2] + b"241301000000Z" + good[t + 15 :]
# Not a certificate at all.
mal["junk"] = b"\x30\x03\x02\x01\x05"

# Tampering that keeps the DER valid: the last byte of the signature, and a byte of the subject.
tampered_sig = flip(good, len(good) - 1)
tampered_tbs = flip(good, good.index(b"leaf.example.test") + 2)

# ---- Output ---------------------------------------------------------------------------

out = []
w = out.append
w("// `std.x509` over crafted certificate chains (" + ("every one, natively only" if mode == "full" else "the cases that need almost no signature check, on both backends") + "). GENERATED by")
w("// `scripts/x509-roots/chains.py " + mode + "`: every certificate is made with the Python")
w("// `cryptography` package, valid from 2024-01-01 to 2034-01-01 unless its case")
w("// says otherwise, and every check is at 2025-06-15. The roots are not in")
w("// Mozilla's store, so each test passes its own store. Real certificates (every")
w("// root of Mozilla's store) are in `vectors/x509_mozilla_roots.osk`.")
w("")
w("use std.x509")
w("")
w(f"fn now() -> Int {{\n    {NOW}\n}}")
w("")
w("fn hex(text: String) -> Bytes {\n    Bytes.from_hex(text).unwrap_or(Bytes.new())\n}")
w("")
QUICK = {
    "a chain of three certificates verifies, ECDSA P-384 over P-256",
    "a chain to a root the store does not hold is UnknownIssuer",
    "a root may be added from PEM, or all roots replaced",
    "PEM text that is not certificates is refused",
    "a certificate's fields read as the certificate says",
    "the host name is checked against the subject alternative names",
    "a name that is not covered is HostMismatch",
    "a wildcard covers one label, only as the whole leftmost label, and not a bare suffix",
    "the common name is never used",
    "the host is checked before the chain",
    "a critical extension that is not understood is refused when parsing",
    "a certificate that is not strictly DER is Malformed",
    "a version that is not v1 or v3 is UnsupportedVersion",
    "a certificate of more than 65536 bytes is Malformed",
    "every error has a message",
    "a trust configuration is the Mozilla roots, more roots, or only the given roots",
    "parse_der and parse_pem read certificates from untrusted input",
}
BODY = r'''// The name of an error kind, which is what the tests pin.
fn kind(error: x509.X509Error) -> String {
    match error {
        x509.X509Error.Malformed => "Malformed"
        x509.X509Error.UnsupportedVersion => "UnsupportedVersion"
        x509.X509Error.UnsupportedCriticalExtension => "UnsupportedCriticalExtension"
        x509.X509Error.UnsupportedAlgorithm => "UnsupportedAlgorithm"
        x509.X509Error.UnknownIssuer => "UnknownIssuer"
        x509.X509Error.BadSignature => "BadSignature"
        x509.X509Error.NotYetValid => "NotYetValid"
        x509.X509Error.Expired => "Expired"
        x509.X509Error.NotCa => "NotCa"
        x509.X509Error.PathLengthExceeded => "PathLengthExceeded"
        x509.X509Error.KeyUsage => "KeyUsage"
        x509.X509Error.ExtendedKeyUsage => "ExtendedKeyUsage"
        x509.X509Error.NameConstraint => "NameConstraint"
        x509.X509Error.DistrustedRoot => "DistrustedRoot"
        x509.X509Error.HostMismatch => "HostMismatch"
        x509.X509Error.LimitExceeded => "LimitExceeded"
    }
}

// The kind of the error parsing `der` gives, or "ok".
fn parse_kind(der: String) -> String {
    match x509.Certificate.parse(hex(der)) {
        Ok(_) => "ok"
        Err(e) => kind(e)
    }
}

fn parsed(der: String) -> List<x509.Certificate> {
    match x509.Certificate.parse(hex(der)) {
        Ok(c) => [c]
        Err(_) => []
    }
}

// A store of the given roots, or the first error making it.
fn store_of(roots: List<String>) -> Result<x509.TrustConfig, x509.X509Error> {
    var store = x509.TrustConfig.only_roots([])
    for root in roots {
        store = store.with_root_der(hex(root))?
    }
    Ok(store)
}

// "ok" with the path length, or the kind of the first error: a store of
// `roots`, the server's `leaf` and `others` in the order sent, `host`.
fn verify(roots: List<String>, leaf: String, others: List<String>, host: String) -> String {
    let store = match store_of(roots) {
        Ok(s) => s
        Err(e) => {
            let reason = kind(e)
            return "store: {reason}"
        }
    }
    let chain = parsed(leaf)
    if chain.len() == 0 {
        let reason = parse_kind(leaf)
        return "parse: {reason}"
    }
    var pool: List<x509.Certificate> = []
    for other in others {
        pool.push(parsed(other).at(0))
    }
    match x509.verify_server(store, chain.at(0), pool, host, now()) {
        Ok(path) => {
            let count = path.len()
            "ok {count}"
        }
        Err(e) => kind(e)
    }
}

fn under_a(leaf: String) -> String {
    verify([root_a()], leaf, [int_a()], "example.test")
}

fn host_a(host: String) -> String {
    verify([root_a()], leaf_a(), [int_a()], host)
}

test "a chain of three certificates verifies, ECDSA P-384 over P-256" {
    t.assert_eq(verify([root_a()], leaf_a(), [int_a()], "example.test"), "ok 2")
}

test "the chain verifies from any order of the server's certificates, with extras and the root" {
    t.assert_eq(verify([root_a()], leaf_a(), [root_a(), root_b(), int_a(), leaf_x()], "example.test"), "ok 2")
    t.assert_eq(verify([root_a()], leaf_a(), [leaf_x(), int_a_other_key(), int_a()], "example.test"), "ok 2")
}

test "the intermediate may be missing only when the root signed the leaf" {
    t.assert_eq(verify([root_a()], leaf_a(), [], "example.test"), "UnknownIssuer")
    t.assert_eq(verify([int_a()], leaf_a(), [], "example.test"), "ok 1")
}

test "RSA chains verify with PKCS#1 v1.5 and PSS, SHA-256, SHA-384 and SHA-512" {
    for leaf in [leaf_b_sha256(), leaf_b_sha384(), leaf_b_sha512(), leaf_b_pss256(), leaf_b_pss384()] {
        t.assert_eq(verify([root_b()], leaf, [int_b()], "rsa.example.test"), "ok 2")
    }
    t.assert_eq(verify([root_b()], leaf_b_sha256(), [int_b_pss(), int_b()], "rsa.example.test"), "ok 2")
}

test "an Ed25519 chain verifies" {
    t.assert_eq(verify([root_c()], leaf_c(), [], "ed.example.test"), "ok 1")
}

test "a self-signed certificate verifies when it is the root" {
    t.assert_eq(verify([self_signed()], self_signed(), [], "self.test"), "ok 1")
    t.assert_eq(verify([root_a()], self_signed(), [], "self.test"), "UnknownIssuer")
}

test "a chain to a root the store does not hold is UnknownIssuer" {
    t.assert_eq(verify([root_a()], leaf_x(), [root_x()], "example.test"), "UnknownIssuer")
    t.assert_eq(verify([root_x()], leaf_x(), [], "example.test"), "ok 1")
    t.assert_eq(verify([], leaf_a(), [int_a(), root_a()], "example.test"), "UnknownIssuer")
}

test "the Mozilla store holds roots and trusts none of the test roots" {
    let store = x509.TrustConfig.default()
    t.assert_eq(store.len() > 100, true)
    match x509.Certificate.parse(hex(root_a())) {
        Ok(root) => t.assert_eq(store.contains(root), false)
        Err(_) => t.assert_eq(true, false)
    }
    match x509.verify_server(store, parsed(leaf_a()).at(0), parsed(int_a()), "example.test", now()) {
        Ok(_) => t.assert_eq(true, false)
        Err(e) => t.assert_eq(kind(e), "UnknownIssuer")
    }
}

test "a root may be added from PEM, or all roots replaced" {
    let body = pem_body(root_a())
    let pem = "-----BEGIN CERTIFICATE-----\n{body}\n-----END CERTIFICATE-----\n"
    match x509.TrustConfig.only_roots([]).with_root_pem(pem) {
        Ok(store) => {
            t.assert_eq(store.len(), 1)
            match x509.verify_server(store, parsed(leaf_a()).at(0), parsed(int_a()), "example.test", now()) {
                Ok(path) => t.assert_eq(path.len(), 2)
                Err(_) => t.assert_eq(true, false)
            }
        }
        Err(_) => t.assert_eq(true, false)
    }
    match x509.TrustConfig.default().with_root_der(hex(root_a())) {
        Ok(store) => t.assert_eq(store.len() > 101, true)
        Err(_) => t.assert_eq(true, false)
    }
}

test "PEM text that is not certificates is refused" {
    t.assert_eq(pem_kind("garbage"), "Malformed")
    t.assert_eq(pem_kind("-----BEGIN CERTIFICATE-----\nAAAA\n"), "Malformed")
    t.assert_eq(pem_kind("-----BEGIN CERTIFICATE-----\n!!!!\n-----END CERTIFICATE-----\n"), "Malformed")
    t.assert_eq(pem_kind(""), "Malformed")
    t.assert_eq(pem_kind("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n"), "Malformed")
}

fn pem_kind(pem: String) -> String {
    match x509.TrustConfig.only_roots([]).with_root_pem(pem) {
        Ok(_) => "ok"
        Err(e) => kind(e)
    }
}

// The base64 of a certificate's DER, in one line.
fn pem_body(der: String) -> String {
    base64(hex(der))
}

fn base64(data: Bytes) -> String {
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".chars()
    var out = ""
    var i = 0
    while i < data.len() {
        let left = data.len() - i
        let b0 = data.at(i).to_int()
        let b1 = if left > 1 { data.at(i + 1).to_int() } else { 0 }
        let b2 = if left > 2 { data.at(i + 2).to_int() } else { 0 }
        let n = b0 * 65536 + b1 * 256 + b2
        let c0 = alphabet.at(n / 262144)
        let c1 = alphabet.at((n / 4096) % 64)
        let c2 = if left > 1 { alphabet.at((n / 64) % 64) } else { "=" }
        let c3 = if left > 2 { alphabet.at(n % 64) } else { "=" }
        out = "{out}{c0}{c1}{c2}{c3}"
        i = i + 3
    }
    out
}

test "a certificate's fields read as the certificate says" {
    match x509.Certificate.parse(hex(leaf_a())) {
        Ok(c) => {
            t.assert_eq(c.is_ca(), false)
            t.assert_eq(c.not_before(), 1704067200)
            t.assert_eq(c.not_after(), 2019686400)
            let names = c.dns_names()
            t.assert_eq(names.len(), 6)
            t.assert_eq(names.at(0).assume_trusted(), "example.test")
            t.assert_eq(names.at(1).assume_trusted(), "www.example.test")
            t.assert_eq(names.at(5).assume_trusted(), "*.sub.example.test")
            t.assert_eq(c.public_key().is_some(), true)
            t.assert_eq(c.to_der(), hex(leaf_a()))
        }
        Err(_) => t.assert_eq(true, false)
    }
    match x509.Certificate.parse(hex(int_a())) {
        Ok(c) => t.assert_eq(c.is_ca(), true)
        Err(_) => t.assert_eq(true, false)
    }
}

fn signed_kind(leaf: x509.Certificate, issuer: x509.Certificate) -> String {
    match leaf.signed_by(issuer) {
        Ok(_) => "ok"
        Err(e) => kind(e)
    }
}

test "signed_by checks the name and the signature" {
    let leaf = parsed(leaf_a()).at(0)
    let issuer = parsed(int_a()).at(0)
    let other = parsed(int_a_other_key()).at(0)
    t.assert_eq(leaf.signed_by(issuer).is_ok(), true)
    t.assert_eq(signed_kind(leaf, other), "BadSignature")
    t.assert_eq(signed_kind(leaf, leaf), "UnknownIssuer")
}

test "the host name is checked against the subject alternative names" {
    t.assert_eq(host_a("example.test"), "ok 2")
    t.assert_eq(host_a("EXAMPLE.test"), "ok 2")
    t.assert_eq(host_a("example.test."), "ok 2")
    t.assert_eq(host_a("www.example.test"), "ok 2")
    t.assert_eq(host_a("a.wild.test"), "ok 2")
    t.assert_eq(host_a("192.0.2.7"), "ok 2")
    t.assert_eq(host_a("2001:db8::1"), "ok 2")
    t.assert_eq(host_a("2001:0db8:0:0:0:0:0:1"), "ok 2")
    t.assert_eq(host_a("a.sub.example.test"), "ok 2")
}

test "a name that is not covered is HostMismatch" {
    t.assert_eq(host_a("other.test"), "HostMismatch")
    t.assert_eq(host_a("foo.example.test"), "HostMismatch")
    t.assert_eq(host_a("wild.test"), "HostMismatch")
    t.assert_eq(host_a("a.b.wild.test"), "HostMismatch")
    t.assert_eq(host_a("192.0.2.8"), "HostMismatch")
    t.assert_eq(host_a("2001:db8::2"), "HostMismatch")
    t.assert_eq(host_a("::1"), "HostMismatch")
    t.assert_eq(host_a(""), "HostMismatch")
    t.assert_eq(host_a(".example.test"), "HostMismatch")
    t.assert_eq(host_a("a..wild.test"), "HostMismatch")
    t.assert_eq(host_a("example.test.."), "HostMismatch")
}

test "a wildcard covers one label, only as the whole leftmost label, and not a bare suffix" {
    // `*.test`, `w*.example.test` and `*.sub.example.test` are in the certificate.
    t.assert_eq(host_a("a.test"), "HostMismatch")
    t.assert_eq(host_a("wx.example.test"), "HostMismatch")
    t.assert_eq(host_a("a.sub.example.test"), "ok 2")
    t.assert_eq(host_a("sub.example.test"), "HostMismatch")
}

test "the common name is never used" {
    t.assert_eq(verify([root_a()], leaf_a_nosan(), [int_a()], "nosan.test"), "HostMismatch")
    t.assert_eq(verify([root_a()], leaf_a_nosan(), [int_a()], "leaf.example.test"), "HostMismatch")
}

test "the host is checked before the chain" {
    t.assert_eq(verify([], leaf_a(), [], "other.test"), "HostMismatch")
}

test "a certificate outside its validity is NotYetValid or Expired" {
    t.assert_eq(under_a(leaf_expired()), "Expired")
    t.assert_eq(under_a(leaf_future()), "NotYetValid")
    t.assert_eq(verify([root_a()], leaf_under_expired(), [int_expired()], "example.test"), "Expired")
}

test "an issuer that is not a CA is NotCa" {
    t.assert_eq(verify([root_a()], leaf_under_noca(), [int_noca()], "example.test"), "NotCa")
    t.assert_eq(verify([root_a()], leaf_under_caflag(), [int_caflag()], "example.test"), "NotCa")
    t.assert_eq(verify([root_a()], leaf_under_ku(), [int_ku()], "example.test"), "NotCa")
}

test "a path longer than a CA allows is PathLengthExceeded" {
    t.assert_eq(verify([root_a()], leaf_p1(), [int_p1(), int_p0()], "example.test"), "PathLengthExceeded")
    t.assert_eq(verify([root_a()], leaf_q1(), [int_q1(), int_q0()], "example.test"), "ok 3")
}

test "key usage and extended key usage are enforced" {
    t.assert_eq(under_a(leaf_ku_enc()), "KeyUsage")
    t.assert_eq(under_a(leaf_eku_client()), "ExtendedKeyUsage")
    t.assert_eq(verify([root_a()], leaf_under_eku(), [int_eku()], "example.test"), "ExtendedKeyUsage")
    t.assert_eq(verify([root_a()], leaf_a_sha512(), [int_a()], "sha512.test"), "ok 2")
}

test "name constraints limit the names in the server's certificate" {
    let ok = verify([root_a()], leaf_nc_ok(), [int_nc()], "a.b.permitted.test")
    t.assert_eq(ok, "ok 2")
    t.assert_eq(verify([root_a()], leaf_nc_other(), [int_nc()], "other.test"), "NameConstraint")
    t.assert_eq(verify([root_a()], leaf_nc_mixed(), [int_nc()], "permitted.test"), "NameConstraint")
    t.assert_eq(verify([root_a()], leaf_nc_excluded(), [int_nc()], "x.bad.permitted.test"), "NameConstraint")
    t.assert_eq(verify([root_a()], leaf_nc_wild_other(), [int_nc()], "a.other.test"), "NameConstraint")
    t.assert_eq(verify([root_a()], leaf_nc_ip_ok(), [int_nc()], "192.0.2.5"), "ok 2")
    t.assert_eq(verify([root_a()], leaf_nc_ip_out(), [int_nc()], "198.51.100.1"), "NameConstraint")
}

test "a name constraint with a leading dot covers only the names below it" {
    t.assert_eq(verify([root_a()], leaf_dot_ok(), [int_nc_dot()], "a.sub.test"), "ok 2")
    t.assert_eq(verify([root_a()], leaf_dot_bare(), [int_nc_dot()], "sub.test"), "NameConstraint")
}

test "a bad signature is BadSignature" {
    t.assert_eq(under_a(tampered_sig()), "BadSignature")
    t.assert_eq(under_a(tampered_tbs()), "BadSignature")
}

test "MD5 and SHA-1 signatures and weak keys are UnsupportedAlgorithm" {
    t.assert_eq(verify([root_b()], leaf_b_sha1(), [int_b()], "rsa.example.test"), "UnsupportedAlgorithm")
    t.assert_eq(verify([root_b()], leaf_b_under_sha1(), [int_b_sha1()], "rsa.example.test"), "UnsupportedAlgorithm")
    t.assert_eq(verify([root_b()], leaf_under_small(), [int_small()], "small.example.test"), "UnsupportedAlgorithm")
}

test "a critical extension that is not understood is refused when parsing" {
    t.assert_eq(parse_kind(leaf_unknown_critical()), "UnsupportedCriticalExtension")
    t.assert_eq(parse_kind(leaf_a()), "ok")
}

test "a root distrusts certificates that became valid after its date" {
    let leaf = parsed(leaf_distrust()).at(0)
    let after = 1735689600  // 2025-01-01
    match x509.TrustConfig.only_roots([]).with_root_distrusting_after(hex(root_a()), after) {
        Ok(store) => {
            match x509.verify_server(store, leaf, parsed(int_a()), "example.test", now()) {
                Ok(_) => t.assert_eq(true, false)
                Err(e) => t.assert_eq(kind(e), "DistrustedRoot")
            }
            // A leaf that became valid before the date is not affected.
            match x509.verify_server(store, parsed(leaf_a()).at(0), parsed(int_a()), "example.test", now()) {
                Ok(path) => t.assert_eq(path.len(), 2)
                Err(_) => t.assert_eq(true, false)
            }
        }
        Err(_) => t.assert_eq(true, false)
    }
}

test "a certificate that is not strictly DER is Malformed" {
    t.assert_eq(parse_kind(bad_trailing()), "Malformed")
    t.assert_eq(parse_kind(bad_truncated()), "Malformed")
    t.assert_eq(parse_kind(bad_empty()), "Malformed")
    t.assert_eq(parse_kind(bad_long_length()), "Malformed")
    t.assert_eq(parse_kind(bad_inner_long_length()), "Malformed")
    t.assert_eq(parse_kind(bad_indefinite()), "Malformed")
    t.assert_eq(parse_kind(bad_ber_boolean()), "Malformed")
    t.assert_eq(parse_kind(bad_not_sequence()), "Malformed")
    t.assert_eq(parse_kind(bad_high_tag()), "Malformed")
    t.assert_eq(parse_kind(bad_bad_time()), "Malformed")
    t.assert_eq(parse_kind(bad_month_13()), "Malformed")
    t.assert_eq(parse_kind(bad_junk()), "Malformed")
}

test "a version that is not v1 or v3 is UnsupportedVersion" {
    t.assert_eq(parse_kind(bad_version_4()), "UnsupportedVersion")
}

fn parse_bytes_kind(der: Bytes) -> String {
    match x509.Certificate.parse(der) {
        Ok(_) => "ok"
        Err(e) => kind(e)
    }
}

test "a certificate of more than 65536 bytes is Malformed" {
    var big = Bytes.new()
    for _ in 0..65537 {
        big.push(0)
    }
    t.assert_eq(parse_bytes_kind(big), "Malformed")
}

test "an issuer is chosen by key identifier too, which also saves signature checks" {
    // 100 intermediates with the right name and the wrong key identifier are skipped unchecked.
    var pool: List<String> = []
    for _ in 0..100 {
        pool.push(int_k_decoy())
    }
    pool.push(int_k_good())
    t.assert_eq(verify([root_a()], leaf_k(), pool, "example.test"), "ok 2")
    t.assert_eq(verify([root_k()], leaf_root_k(), [], "example.test"), "ok 1")
    t.assert_eq(verify([root_k()], leaf_root_k_wrong(), [], "example.test"), "UnknownIssuer")
}

test "the work done on one validation is bounded at 64 signature checks" {
    // Each decoy has the issuer's name and no key identifier, so each costs a check. The path needs two
    // checks of its own: 62 decoys leave room for them, and 63 do not.
    var pool: List<String> = []
    for _ in 0..62 {
        pool.push(int_a_other_key())
    }
    pool.push(int_a())
    t.assert_eq(verify([root_a()], leaf_a(), pool, "example.test"), "ok 2")
    var more: List<String> = [int_a_other_key()]
    for d in pool {
        more.push(d)
    }
    t.assert_eq(verify([root_a()], leaf_a(), more, "example.test"), "LimitExceeded")
}

test "a path is at most eight certificates before the root" {
    var seven: List<String> = []
    for d in [deep_1(), deep_2(), deep_3(), deep_4(), deep_5(), deep_6(), deep_7()] {
        seven.push(d)
    }
    t.assert_eq(verify([root_a()], leaf_deep7(), seven, "example.test"), "ok 8")
    let all = [deep_1(), deep_2(), deep_3(), deep_4(), deep_5(), deep_6(), deep_7(), deep_8()]
    t.assert_eq(verify([root_a()], leaf_deep8(), all, "example.test"), "UnknownIssuer")
}

test "the server's certificate must name server authentication" {
    t.assert_eq(under_a(leaf_no_eku()), "ExtendedKeyUsage")
}

test "a CA's name constraints apply to the certificates below it, not only the leaf" {
    t.assert_eq(verify([root_a()], leaf_under_sub_ok(), [sub_ok(), int_nc()], "a.permitted.test"), "ok 3")
    t.assert_eq(verify([root_a()], leaf_under_sub_bad(), [sub_bad(), int_nc()], "a.permitted.test"), "NameConstraint")
}

test "a trust configuration is the Mozilla roots, more roots, or only the given roots" {
    let default = x509.TrustConfig.default()
    t.assert_eq(default.len() > 100, true)
    let roots = parsed(root_a())
    t.assert_eq(x509.TrustConfig.only_roots(roots).len(), 1)
    t.assert_eq(default.with_roots(roots).len(), default.len() + 1)
    t.assert_eq(x509.TrustConfig.only_roots(roots).contains(roots.at(0)), true)
    t.assert_eq(default.contains(roots.at(0)), false)
}

test "parse_der and parse_pem read certificates from untrusted input" {
    match x509.parse_der(hex(leaf_a())) {
        Ok(c) => t.assert_eq(c.to_der(), hex(leaf_a()))
        Err(_) => t.assert_eq(true, false)
    }
    let one = pem_body(root_a())
    let two = pem_body(int_a())
    let pem = "-----BEGIN CERTIFICATE-----\n{one}\n-----END CERTIFICATE-----\n\n-----BEGIN CERTIFICATE-----\n{two}\n-----END CERTIFICATE-----\n"
    match x509.parse_pem(pem) {
        Ok(list) => {
            t.assert_eq(list.len(), 2)
            t.assert_eq(list.at(1).is_ca(), true)
        }
        Err(_) => t.assert_eq(true, false)
    }
    match x509.parse_pem("") {
        Ok(_) => t.assert_eq(true, false)
        Err(e) => t.assert_eq(kind(e), "Malformed")
    }
}

test "every error has a message" {
    t.assert_eq(x509.X509Error.HostMismatch.to_string(), "the certificate is not for this host name")
    t.assert_eq(x509.X509Error.UnknownIssuer.to_string(), "the certificate does not chain to a trusted root")
}
'''

ITEMS = re.split(r'\n\n(?=test "|fn |// )', BODY)

QUICK_HOST = """fn host_a(host: String) -> String {
    if parsed(leaf_a()).at(0).matches_host(host) {
        "ok 2"
    } else {
        "HostMismatch"
    }
}"""
QUICK.add("the Mozilla store holds roots and trusts none of the test roots")

def wanted(item):
    if mode == "full" or not item.startswith("test "):
        return True
    return item.split('" {')[0][len('test "'):] in QUICK

kept = [item for item in ITEMS if wanted(item)]
if mode == "quick":
    kept = [QUICK_HOST if item.startswith("fn host_a(") else item for item in kept]
text = "\n\n".join(kept)
data = {label: der for label, der in certs.items()}
data.update({f"bad_{label}": der for label, der in mal.items()})
data["tampered_sig"] = tampered_sig
data["tampered_tbs"] = tampered_tbs
for label, der in data.items():
    if re.search(rf"\b{label}\(\)", text):
        w(f"fn {label}() -> String {{\n    \"{der.hex()}\"\n}}\n")
w(text)

sys.stdout.write("\n".join(out))
