#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Write the SPDX SBOM of a release.
#
#   SOURCE_DATE_EPOCH=<seconds> scripts/release/sbom.sh <tag> <Cargo.lock>
#
# Prints an SPDX 2.3 JSON document on standard output. It lists every
# package in <Cargo.lock>, the lock of the tagged commit that the release
# is built from with `cargo build --locked`, so it names the exact crates
# (and the checksum Cargo recorded for each registry crate) that the
# binaries are made of. `oskac` itself is the package the document
# describes. It reads no network and runs no build tool, only awk and jq.
#
# Same input, same bytes: the creation time is SOURCE_DATE_EPOCH (the
# tag's commit time, as for the image) and the packages keep the lock's
# order. The document namespace names the release, so two releases never
# share one.
#
# assemble.sh writes the result into the release folder as oskac.spdx.json,
# and SHA256SUMS lists it, so the release signature covers it.

set -euo pipefail

die() {
  echo "sbom.sh: $*" >&2
  exit 1
}

[ $# -eq 2 ] || die "usage: sbom.sh <tag> <Cargo.lock>"
tag=$1
lock=$2

[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$ ]] ||
  die "tag '$tag' is not of the form vX.Y.Z or vX.Y.Z-pre"
[ -f "$lock" ] || die "no $lock"
[[ ${SOURCE_DATE_EPOCH:-} =~ ^[0-9]+$ ]] || die "SOURCE_DATE_EPOCH must be set to a number of seconds"

created=$(date -u -d "@$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ 2>/dev/null ||
  date -u -r "$SOURCE_DATE_EPOCH" +%Y-%m-%dT%H:%M:%SZ) || die "cannot format SOURCE_DATE_EPOCH"

# One line per fact, tab separated, in the lock's order:
#   P <name> <version> <source or -> <checksum or ->
#   D <name> <version> <dependency as the lock writes it>
rows=$(awk '
  function value(line,   v) {
    v = line
    sub(/^[^=]*= *"/, "", v)
    sub(/"[ \t]*$/, "", v)
    return v
  }
  function flush() {
    if (name != "") {
      printf "P\t%s\t%s\t%s\t%s\n", name, version, (source == "" ? "-" : source), (checksum == "" ? "-" : checksum)
      for (i = 1; i <= n; i++) printf "D\t%s\t%s\t%s\n", name, version, deps[i]
    }
    name = ""; version = ""; source = ""; checksum = ""; n = 0; indeps = 0
  }
  /^\[\[package\]\]/ { flush(); inpkg = 1; next }
  /^\[/ { flush(); inpkg = 0; next }
  !inpkg { next }
  indeps {
    if ($0 ~ /^\]/) { indeps = 0; next }
    d = $0
    sub(/^[ \t]*"/, "", d)
    sub(/",?[ \t]*$/, "", d)
    deps[++n] = d
    next
  }
  /^name = / { name = value($0); next }
  /^version = / { version = value($0); next }
  /^source = / { source = value($0); next }
  /^checksum = / { checksum = value($0); next }
  /^dependencies = \[/ { indeps = 1; next }
  END { flush() }
' "$lock")

[ -n "$rows" ] || die "$lock lists no package"

printf '%s\n' "$rows" | jq -R -s -e \
  --arg tag "$tag" --arg created "$created" '
  def spdx_id($i; $name): "SPDXRef-Package-\($i)-\($name | gsub("[^A-Za-z0-9.-]"; "-"))";
  (split("\n") | map(select(. != "") | split("\t"))) as $rows
  | [$rows[] | select(.[0] == "P")] as $p
  | [$rows[] | select(.[0] == "D")] as $d
  | ($p | to_entries | map({
      id: spdx_id(.key; .value[1]),
      name: .value[1], version: .value[2], source: .value[3], checksum: .value[4]
    })) as $pkgs
  | ($pkgs | map(select(.name == "oskac"))) as $root
  | if ($root | length) != 1 then error("Cargo.lock must list exactly one package named oskac") else . end
  # A dependency is "name", "name version" or "name version (source)".
  | def resolve($dep):
      ($dep | split(" ")) as $w
      | [$pkgs[] | select(.name == $w[0] and (($w | length) < 2 or .version == $w[1]))] as $m
      | if ($m | length) == 1 then $m[0].id
        else error("dependency \($dep) does not name exactly one package in Cargo.lock") end;
  {
    spdxVersion: "SPDX-2.3",
    dataLicense: "CC0-1.0",
    SPDXID: "SPDXRef-DOCUMENT",
    name: "oskac \($tag)",
    documentNamespace: "https://oskalang.dev/releases/\($tag)/oskac.spdx.json",
    creationInfo: {created: $created, creators: ["Tool: oska-release-sbom"]},
    documentDescribes: [$root[0].id],
    packages: [$pkgs[] | {
      SPDXID: .id,
      name: .name,
      versionInfo: .version,
      downloadLocation: (if .source == "-" then "https://github.com/aparsons/oska"
                         elif (.source | startswith("registry+")) then "https://crates.io/crates/\(.name)/\(.version)/download"
                         else (.source | sub("^[a-z]+\\+"; "")) end),
      filesAnalyzed: false,
      licenseConcluded: "NOASSERTION",
      licenseDeclared: (if .source == "-" then "Apache-2.0" else "NOASSERTION" end),
      copyrightText: "NOASSERTION",
      externalRefs: [{
        referenceCategory: "PACKAGE-MANAGER",
        referenceType: "purl",
        referenceLocator: "pkg:cargo/\(.name)@\(.version)"
      }]
    } + (if .checksum == "-" then {} else {checksums: [{algorithm: "SHA256", checksumValue: .checksum}]} end)],
    relationships: (
      [{spdxElementId: "SPDXRef-DOCUMENT", relationshipType: "DESCRIBES", relatedSpdxElement: $root[0].id}]
      + [$d[] | . as $row
         | ($pkgs | map(select(.name == $row[1] and .version == $row[2])) | map(.id)) as $owners
         | $row[3] as $dep
         | {spdxElementId: ($owners[0]), relationshipType: "DEPENDS_ON", relatedSpdxElement: resolve($dep)}]
    )
  }' || die "could not write the SBOM from $lock"
