#!/usr/bin/env python3
"""Generate crates/oska-check/src/std/x509_roots.osk from Mozilla's certdata.txt.

    python3 -I scripts/x509-roots/generate.py CERTDATA.txt NSS_3_134_RTM 2.94 > crates/oska-check/src/std/x509_roots.osk

CERTDATA.txt is NSS's `lib/ckfw/builtins/certdata.txt` at a release tag
(`NSS_3_134_RTM` here), the file Mozilla's root store is published as, and 2.94
is `NSS_BUILTINS_LIBRARY_VERSION` in `nssckbi.h` at the same tag. Both go into the
generated module, which `scripts/x509-roots/verify.sh` regenerates from the tag. The script keeps the roots trusted for
TLS server authentication (`CKA_TRUST_SERVER_AUTH` is
`CKT_NSS_TRUSTED_DELEGATOR`) and, for each, its subject name, its
SubjectPublicKeyInfo and its "distrust after" date. It uses only the Python
standard library and reads no network: the same file gives the same output.
See docs/guide/x509.md ("Updating the roots") for the whole update step.
"""

import calendar
import hashlib
import re
import sys


def unoctal(lines):
    """The bytes a MULTILINE_OCTAL value spells."""
    out = bytearray()
    for line in lines:
        for match in re.finditer(r"\\([0-7]{3})", line):
            out.append(int(match.group(1), 8))
    return bytes(out)


def objects(text):
    """Every object of certdata.txt as a dict of attribute name to (type, value)."""
    result = []
    current = None
    lines = iter(text.splitlines())
    started = False
    for line in lines:
        if not started:
            started = line == "BEGINDATA"
            continue
        if line.startswith("#") or not line.strip():
            continue
        name, _, rest = line.partition(" ")
        kind, _, value = rest.partition(" ")
        if name == "CKA_CLASS":
            current = {}
            result.append(current)
        if kind == "MULTILINE_OCTAL":
            body = []
            for inner in lines:
                if inner == "END":
                    break
                body.append(inner)
            current[name] = (kind, unoctal(body))
        else:
            current[name] = (kind, value)
    return result


def der_item(data, pos):
    """The (tag, start of content, end of content) of the DER item at pos."""
    tag = data[pos]
    length = data[pos + 1]
    pos += 2
    if length >= 0x80:
        count = length & 0x7F
        length = int.from_bytes(data[pos : pos + count], "big")
        pos += count
    return tag, pos, pos + length


def subject_spki_ski(cert):
    """The subject Name, the SubjectPublicKeyInfo (both whole) and the subject key identifier (b"" if none)."""
    _, start, _ = der_item(cert, 0)  # Certificate
    _, start, _ = der_item(cert, start)  # TBSCertificate
    pos = start
    tag, _, end = der_item(cert, pos)
    if tag == 0xA0:  # version
        pos = end
    fields = []
    for _ in range(6):  # serial, signature, issuer, validity, subject, spki
        _, _, end = der_item(cert, pos)
        fields.append(cert[pos:end])
        pos = end
    ski = b""
    tbs_end = der_item(cert, der_item(cert, 0)[1])[2]
    while pos < tbs_end:
        tag, content, end = der_item(cert, pos)
        if tag == 0xA3:  # extensions
            _, seq_start, seq_end = der_item(cert, content)
            ext = seq_start
            while ext < seq_end:
                _, e_start, e_end = der_item(cert, ext)
                _, o_start, o_end = der_item(cert, e_start)  # the OID
                if cert[o_start:o_end] == bytes.fromhex("551d0e"):
                    tag, _, after = der_item(cert, o_end)
                    value = after if tag == 0x01 else o_end  # skip a critical BOOLEAN
                    _, v_start, _ = der_item(cert, value)  # the OCTET STRING holding the extension
                    _, i_start, i_end = der_item(cert, v_start)  # the keyIdentifier OCTET STRING
                    ski = cert[i_start:i_end]
                ext = e_end
        pos = end
    return fields[4], fields[5], ski


def unix_time(utc):
    """Seconds since 1970 of a UTCTime YYMMDDHHMMSSZ (NSS writes years 1950 to 2049)."""
    match = re.fullmatch(r"(\d\d)(\d\d)(\d\d)(\d\d)(\d\d)(\d\d)Z", utc)
    if not match:
        raise SystemExit(f"bad distrust-after time {utc!r}")
    yy, mo, dd, hh, mi, ss = (int(g) for g in match.groups())
    year = 1900 + yy if yy >= 50 else 2000 + yy
    return calendar.timegm((year, mo, dd, hh, mi, ss))


def osk_string(text):
    if re.search(r"[\x00-\x1f\x7f\"\\{}]", text):
        raise SystemExit(f"label {text!r} needs escaping; extend generate.py")
    return '"' + text + '"'


def trusted_roots(objs):
    """The roots trusted for TLS server authentication, as (label, certificate DER, subject, spki, distrust-after, subject key identifier)."""
    trusted = {}
    for obj in objs:
        if obj.get("CKA_CLASS", ("", ""))[1] == "CKO_NSS_TRUST":
            if obj["CKA_TRUST_SERVER_AUTH"][1] == "CKT_NSS_TRUSTED_DELEGATOR":
                trusted[(obj["CKA_ISSUER"][1], obj["CKA_SERIAL_NUMBER"][1])] = True
    roots = []
    for obj in objs:
        if obj.get("CKA_CLASS", ("", ""))[1] != "CKO_CERTIFICATE":
            continue
        if (obj["CKA_ISSUER"][1], obj["CKA_SERIAL_NUMBER"][1]) not in trusted:
            continue
        label = obj["CKA_LABEL"][1].strip('"')
        subject, spki, ski = subject_spki_ski(obj["CKA_VALUE"][1])
        if subject != obj["CKA_SUBJECT"][1]:
            raise SystemExit(f"{label}: the subject in the certificate differs from CKA_SUBJECT")
        distrust = obj.get("CKA_NSS_SERVER_DISTRUST_AFTER", ("", ""))
        after = unix_time(distrust[1].decode("ascii")) if distrust[0] == "MULTILINE_OCTAL" else 0
        roots.append((label, obj["CKA_VALUE"][1], subject, spki, after, ski))
    roots.sort(key=lambda r: (r[0].lower(), r[2]))
    if len({r[2] + r[3] for r in roots}) != len(roots):
        raise SystemExit("two roots have the same subject and key")
    return roots


def main():
    if len(sys.argv) != 4 or not re.fullmatch(r"NSS_\d+_\d+(_\d+)?_RTM", sys.argv[2]) or not re.fullmatch(r"\d+\.\d+", sys.argv[3]):
        raise SystemExit(__doc__)
    raw = open(sys.argv[1], "rb").read()
    release, builtins = sys.argv[2], sys.argv[3]
    digest = hashlib.sha256(raw).hexdigest()
    roots = [(l, sub, spki, after, ski) for (l, _, sub, spki, after, ski) in trusted_roots(objects(raw.decode("utf-8")))]

    out = []
    out.append("// SPDX-License-Identifier: Apache-2.0")
    out.append("// GENERATED by scripts/x509-roots/generate.py: do not edit by hand.")
    out.append(f"// Source: Mozilla's certdata.txt at NSS release {release} (builtins {builtins}), SHA-256 {digest}.")
    out.append(f"// {len(roots)} roots trusted for TLS server authentication.")
    out.append("// To update: docs/guide/x509.md, \"Updating the roots\".")
    out.append("//")
    out.append("// `std.x509`'s pinned root store, as trust anchors: each root's name, its")
    out.append("// subject (a DER `Name`), its key (a DER `SubjectPublicKeyInfo`), both in")
    out.append("// hexadecimal, and Mozilla's \"distrust after\" time in seconds since 1970")
    out.append("// (0 for none): a server certificate that became valid after it is not")
    out.append("// trusted through this root. `ski` is the root's subject key identifier in")
    out.append("// hexadecimal, empty if it has none: it picks the right root when two share a name.")
    out.append("")
    out.append("pub struct Root { pub name: String, pub subject: String, pub key: String, pub ski: String, pub distrust_after: Int }")
    out.append("")
    out.append("// The NSS release tag the roots were taken from, and that release's `NSS_BUILTINS_LIBRARY_VERSION`.")
    out.append(f'pub fn nss_release() -> String {{\n    "{release}"\n}}')
    out.append("")
    out.append(f'pub fn nss_builtins_version() -> String {{\n    "{builtins}"\n}}')
    out.append("")
    out.append("pub fn roots() -> List<Root> {")
    out.append("    [")
    for label, subject, spki, after, ski in roots:
        out.append(
            f"        Root(name: {osk_string(label)}, subject: \"{subject.hex()}\", key: \"{spki.hex()}\", ski: \"{ski.hex()}\", distrust_after: {after}),"
        )
    out.append("    ]")
    out.append("}")
    sys.stdout.write("\n".join(out) + "\n")


if __name__ == "__main__":
    main()
