# Hashing, HMAC and verifying SSH signatures

Oska's `std` has four modules for the cryptography a program needs to check what it downloaded: `std.sha2` hashes, `std.hmac` authenticates a message under a secret key, `std.ed25519` verifies an Ed25519 signature, and `std.sshsig` reads OpenSSH public keys and verifies the signatures `ssh-keygen -Y sign` writes. The toolchain uses the same code to verify its own releases. Every program on this page is checked by `oskac` when the documentation is tested, and the output shown is what `oskac run` prints.

## What this crypto is, and is not

This code is hand-written, in Oska, and has not been audited by anyone outside the project. [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md) accepts that on purpose: the primitives are Oska source, with no Rust implementation behind them, so one body of code runs on the interpreter and in every native executable. Its assurance is what the project can show:

- Each module is held by published test vectors, run as Oska tests on both backends on every supported platform: FIPS 180-4 and NIST CAVP for `std.sha2`, RFC 4231 and Wycheproof for `std.hmac`, RFC 8032 and Wycheproof for `std.ed25519`, and signatures made by `ssh-keygen` for `std.sshsig`, including the published releases' own `SHA256SUMS.sig`.
- The checker treats the modules as user code. They contain no `expose()` and no `.assume_trusted()`, so `oskac audit` lists no audit site inside them, and every function is a pure function.
- The secret forms (`sha256_secret` and `hmac.sha256`) are written over secret integers, so the checker refuses a secret-dependent branch or memory address and the native backend checks the code it emits. That is the whole of the timing story: nothing measures time, and secret memory is not zeroed. See [secrets](secrets.md).

Verification handles only public data (a message, a key and a signature), so it is not written to run in constant time. There is no signing, no key generation and no random number source in these modules.

## Hashing

`sha2.sha256(data)` and `sha2.sha512(data)` give the digest of a `Bytes` as 32 or 64 `Bytes`. They take trusted or untrusted bytes, such as a file read with `read_bytes`. The digest is trusted whatever went in: it has a fixed length and the input cannot shape it into anything structural, so there is nothing to vouch for. Print a digest with `to_hex()`, since `Bytes` has no `Display`.

For data that arrives in pieces, `sha2.Sha256.new()` and `sha2.Sha512.new()` make a hasher, `update(data)` adds to it, and `finish()` gives the digest. A hasher is a value like any other in Oska: copying one forks its state, so a prefix can be hashed once and finished several ways.

```oska
use std.sha2

fn main(io: Stdout) -> Result<(), IoError> {
    let whole = sha2.sha256("abc".to_bytes()).to_hex()
    io.println("sha-256 of abc: {whole}")?
    var hasher = sha2.Sha256.new()
    hasher.update("a".to_bytes())
    hasher.update("bc".to_bytes())
    let pieces = hasher.finish().to_hex()
    io.println("in pieces:      {pieces}")?
    let short = sha2.sha512("abc".to_bytes()).slice(0, 8).to_hex()
    io.println("sha-512 starts: {short}")?
    Ok(())
}
```

```console cmd=run
sha-256 of abc: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
in pieces:      ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
sha-512 starts: ddaf35a193617aba
```

To hash key material, `sha2.sha256_secret(data)` takes a `Secret<Bytes>` and gives a `Secret<Bytes>`, and `sha2.Sha256Secret` is its hasher. Only the length of the data is public. There is no secret SHA-512.

## HMAC

`hmac.sha256(key, message)` gives the HMAC-SHA-256 tag of a message under a `Secret<Bytes>` key, as a `Secret<Bytes>`. A key made from environment text needs no `expose()`: `env.secret("NAME")` gives a `Secret<String>`, and `.to_bytes()` on it gives its UTF-8 as a `Secret<Bytes>`. A tag is secret, so the checker keeps it out of text and refuses `==` on it.

To check a tag that someone else sent, call `hmac.verify_sha256(key, message, tag)`. It compares in constant time and gives a public `Bool`, which is `false` for a tag that is not 32 bytes. It is not an audit site. The message and the tag may be untrusted.

```oska
use std.hmac

fn main(io: Stdout) -> Result<(), IoError> {
    let key = Secret.new("Jefe".to_bytes())
    let message = "what do ya want for nothing?".to_bytes()
    let tag = hmac.sha256(key, message)
    let received = tag.expose()
    let hex = received.to_hex()
    io.println("tag: {hex}")?
    let key_again = Secret.new("Jefe".to_bytes())
    if hmac.verify_sha256(key_again, message, received) {
        io.println("tag accepted")?
    } else {
        io.println("tag refused")?
    }
    let forged = Bytes.zeroed(32)
    if hmac.verify_sha256(key_again, message, forged) {
        io.println("forged tag accepted")?
    } else {
        io.println("forged tag refused")?
    }
    Ok(())
}
```

```console cmd=run
tag: 5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843
tag accepted
forged tag refused
```

The `expose()` in this program is its audit site, and `oskac audit` lists it: the program chose to make the tag public to print it. A program that only checks tags never needs one. Compare two tags with `verify_sha256`, never with `==`:

```oska error=E0351
use std.hmac

fn same(key: Secret<Bytes>, message: Bytes, other: Secret<Bytes>) -> Bool {
    hmac.sha256(key, message) == other
}
```

## Ed25519

`ed25519.PublicKey.from_bytes(bytes)` reads a 32-byte public key and gives `None` unless the bytes are a canonical encoding of a point on the curve that is not of small order. `ed25519.verify(key, message, signature)` gives `true` only for a valid 64-byte signature as RFC 8032 section 5.1.7 defines one, with the strict checks (a canonical `R`, and `S` below the group order) that OpenSSH and the Wycheproof vectors expect. Every signature `ssh-keygen` makes verifies. The key, the message and the signature may all be untrusted.

```oska
use std.ed25519

fn hex(text: String) -> Bytes {
    Bytes.from_hex(text).unwrap_or(Bytes.new())
}

fn main(io: Stdout) -> Result<(), IoError> {
    // RFC 8032 section 7.1, test 1: the empty message.
    let key_bytes = hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
    let signature = hex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b")
    match ed25519.PublicKey.from_bytes(key_bytes) {
        Some(key) => {
            let empty = if ed25519.verify(key, Bytes.new(), signature) { "valid" } else { "invalid" }
            let other = if ed25519.verify(key, "x".to_bytes(), signature) { "valid" } else { "invalid" }
            io.println("signature of the empty message: {empty}")?
            io.println("signature of another message:   {other}")?
        }
        None => io.println("not a public key")?
    }
    Ok(())
}
```

```console cmd=run
signature of the empty message: valid
signature of another message:   invalid
```

`ed25519` verifies; it does not sign, make keys or read OpenSSH's private key files.

## Verifying an SSH signature

`ssh-keygen -Y sign` signs a file with an SSH key and writes an armored `.sig` file. `sshsig` checks one against the key you trust. The pieces:

- `sshsig.PublicKey.parse(line)` reads an `ssh-ed25519` public key line, as in a `.pub` file: the type, the base64 key and an optional comment. It gives `Result<PublicKey, SshsigError>`. Another key type is `UnsupportedAlgorithm`. `key.fingerprint()` is the `SHA256:` text `ssh-keygen -l` prints, and `key.comment()` is the comment.
- `sshsig.verify(signature, message, namespace, key)` gives `Ok(())` when the armored `signature` is the key's signature of `message` in `namespace`, and an `SshsigError` otherwise. `SshsigError` is `Display`, with one stable line for each of its cases: `Armor`, `Malformed`, `UnsupportedVersion`, `UnsupportedAlgorithm`, `WrongKey`, `WrongNamespace` and `BadSignature`.

Which key to trust and which namespace to accept are your program's policy, so both are structural positions: the key line and the namespace are trusted text, and untrusted text there is `E0300`. Text you read from a file is untrusted. A program that reads the key from a file it ships with vouches for it with `.assume_trusted()`, an audit site that `oskac audit` lists, and that is where a reviewer looks at which key the program trusts. The signature and the message are untrusted, and a hostile signature gives an error, never a panic. `sshsig` does not model `allowed_signers` files, principals or certificates: the caller passes the one key it trusts.

The namespace is part of what was signed: the signer chose it with `ssh-keygen -n`, and a signature made for one purpose does not verify for another. The program below verifies a message that this command signed:

```text
ssh-keygen -Y sign -f test_key -n oska-test -O hashalg=sha256 message.txt
```

It then checks that a changed message, another namespace and another key are each refused.

```oska
use std.sshsig

fn main(io: Stdout) -> Result<(), IoError> {
    let key_line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJkiHKcB7olfSbFctH77PhH1wdg/QVxvI544xAB2AS+g oska-test-key"
    let armored = "-----BEGIN SSH SIGNATURE-----\nU1NIU0lHAAAAAQAAADMAAAALc3NoLWVkMjU1MTkAAAAgmSIcpwHuiV9JsVy0fvs+EfXB2D\n9BXG8jnjjEAHYBL6AAAAAJb3NrYS10ZXN0AAAAAAAAAAZzaGEyNTYAAABTAAAAC3NzaC1l\nZDI1NTE5AAAAQJ1UUJ30kvyHaM/11D7QzgjaxkCWZThrZ1BStim02EVIx+JxQLdlVZCCTJ\n7eXxDZlD1ZDxgs1hYTIwbx1GvEHwA=\n-----END SSH SIGNATURE-----\n"
    let message = "Oska signs this message for the sshsig tests.\n".to_bytes()
    match sshsig.PublicKey.parse(key_line) {
        Ok(key) => {
            let comment = key.comment()
            let fingerprint = key.fingerprint()
            let prefixed = if fingerprint.starts_with("SHA256:") { "yes" } else { "no" }
            let size = fingerprint.len()
            io.println("key {comment}, fingerprint prefixed {prefixed}, {size} characters")?
            match sshsig.verify(armored, message, "oska-test", key) {
                Ok(_) => io.println("signature verified")?
                Err(e) => io.println("refused: {e}")?
            }
            match sshsig.verify(armored, "Another message.\n".to_bytes(), "oska-test", key) {
                Ok(_) => io.println("signature verified")?
                Err(e) => io.println("changed message: {e}")?
            }
            match sshsig.verify(armored, message, "file", key) {
                Ok(_) => io.println("signature verified")?
                Err(e) => io.println("other namespace: {e}")?
            }
        }
        Err(e) => io.println("bad key: {e}")?
    }
    Ok(())
}
```

```console cmd=run
key oska-test-key, fingerprint prefixed yes, 50 characters
signature verified
changed message: the signature does not verify
other namespace: the signature was made for a different namespace
```

### Verifying a file on disk

The same check on files: read the file as bytes, so no newline is translated, read the signature as text, and verify. This is the shape of how a release is checked: `SHA256SUMS` is signed, and the digests in it then vouch for the other files.

```oska
use std.sha2
use std.sshsig

fn verify_release(dir: DirRead, key: sshsig.PublicKey) -> Result<String, IoError> {
    let sums = dir.read_bytes(Path.new("SHA256SUMS"))?
    let signature = dir.read_to_string(Path.new("SHA256SUMS.sig"))?
    match sshsig.verify(signature, sums, "oska-release", key) {
        Ok(_) => {
            let digest = sha2.sha256(sums).to_hex()
            Ok("SHA256SUMS is signed, and hashes to {digest}")
        }
        Err(e) => Ok("not verified: {e}")
    }
}

fn main(fs: Fs, io: Stdout) -> Result<(), IoError> {
    let key_line = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIJkiHKcB7olfSbFctH77PhH1wdg/QVxvI544xAB2AS+g trusted"
    match sshsig.PublicKey.parse(key_line) {
        Ok(key) => {
            let dir = fs.read_only().restrict_to("release")
            let line = verify_release(dir, key)?
            io.println(line)?
        }
        Err(e) => io.println("bad key: {e}")?
    }
    Ok(())
}
```

The release signature of the Oska toolchain itself is made in the namespace `oska-release` with the key published as `oska-release.pub`, and its fingerprint is in the [releasing guide](releasing.md). Do not take the key out of the same download as the file it signs.

## What to remember

- Hash with `sha2`, authenticate with `hmac`, verify with `ed25519` and `sshsig`. Use the secret forms for secret data and `verify_sha256` for tags.
- The code is hand-written and unaudited, and held by published test vectors on every platform. Weigh it as [ADR 0012](../adr/0012-tls-git-and-crypto-are-written-in-oska.md) does.
- The key and the namespace you trust are policy: trusted text, and an audit site where a program vouches for text it read.
- The API of each module is in the API reference on the site, and `oskac doc std.sha2` (or `std.hmac`, `std.ed25519`, `std.sshsig`) prints it.
