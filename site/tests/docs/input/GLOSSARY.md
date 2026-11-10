# Oska

Terms of the security model.

## Authority

**Capability**:
An unforgeable value that grants authority.
_Avoid_: permission, token

**Attenuation**:
Producing a narrower capability, as `fs.restrict_to("docs")` does. See the [design](docs/design.md#capabilities).
