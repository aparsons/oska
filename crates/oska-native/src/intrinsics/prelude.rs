// SPDX-License-Identifier: Apache-2.0
//! The prelude intrinsics and how each is lowered.

/// How a prelude intrinsic is lowered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prelude {
    /// A call of the `std.rt` function of this name.
    Runtime(&'static str),
    /// A call of the `std.rt` function of this name, which writes its
    /// argument 1 and a newline to stdout or stderr: under `--max-output`
    /// the bytes are counted first (`Body::spend_output`). Every prelude
    /// intrinsic whose function can write the program's text must be one,
    /// which a test checks.
    WriteLine(&'static str),
    /// `Approver.confirm`, and nothing else: `std.rt`'s `approval_prompt`
    /// writes the question first, from arguments 1 and 2, whose bytes are
    /// counted under `--max-output` (`Body::spend_output`), and then
    /// `approver_confirm`, given the arguments and the question, writes it
    /// to stderr (or records the question, in a test) and reads the
    /// answer.
    Confirm,
    /// Its one argument is its result, as `String.to_string`'s is.
    Identity,
    /// Its result is field `n` of its struct receiver.
    Field(usize),
    /// A call of the `std.rt` function of this name, which gives the word
    /// 0 (or, for [`Check::Repeat`], also -1) in place of its result when
    /// the call is a bug, for the generated code to panic as `check` says.
    Checked(&'static str, Check),
    /// `Bytes.at` (`width` 1, panicking with `P0002`) or an endian read
    /// of `width` bytes: `std.rt`'s `bytes_readable` first, which gives
    /// 0 when the bytes do not all lie inside the buffer, for the
    /// generated code to panic, then `bytes_read`.
    ///
    /// A `secret` one, `Secret<Bytes>`'s, calls `bytes_read_secret`
    /// instead, which marks each byte it reads secret
    /// (`constant_time.rs`); `Bytes`' own gives public bytes.
    ByteRead { width: u8, big: bool, secret: bool },
    /// A mutating method of `Bytes`: a call of the `std.rt` function of
    /// this name with the receiver's object (and the place's reference to
    /// it) and the other arguments, which gives the object the place
    /// holds afterwards, or the word 0, changing nothing, for the
    /// generated code to panic as `check` says.
    Mutating(&'static str, Check),
}

/// The panics a [`Prelude::Checked`] call may end in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// `String.slice`: `P0035` on the word 0.
    Slice,
    /// `String.repeat`: `P0036` on the word 0, `P0037` on -1.
    Repeat,
    /// `Env.secret`: `P0020` on the word 0.
    Secret,
    /// `Bytes.zeroed`: `P0042` on the word 0, `P0043` on -1.
    Zeroed,
    /// `Random.secure_bytes`: as `Zeroed`, the capability first.
    SecureBytes,
    /// `Bytes.slice`: `P0042` on the word 0.
    BytesSlice,
    /// `Bytes.set`: `P0002` on the word 0.
    BytesIndex,
    /// A `Bytes` push of this many bytes: `P0043` on the word 0.
    BytesGrow(u8),
    /// `Bytes.append`: `P0043` on the word 0.
    BytesAppend,
    /// `ct_select` between two `Bytes`: `P0042` on the word 0.
    BytesSelect,
}

/// The prelude intrinsics the native backend implements, by the name the
/// checked program calls them by. A call of any other is refused (`E0390`).
pub const PRELUDE: &[(&str, Prelude)] = &[
    ("Stdout.println", Prelude::WriteLine("stdout_println")),
    ("Stderr.println", Prelude::WriteLine("stderr_println")),
    ("Stdin.read_line", Prelude::Runtime("stdin_read_line")),
    ("Args.get", Prelude::Runtime("args_get")),
    ("Env.get", Prelude::Runtime("env_get")),
    ("Env.secret", Prelude::Checked("env_secret", Check::Secret)),
    ("Secret.expose", Prelude::Field(0)),
    (
        "Secret.constant_time_eq",
        Prelude::Runtime("secret_constant_time_eq"),
    ),
    (
        "Secret.constant_time_eq_secret",
        Prelude::Runtime("secret_constant_time_eq_secret"),
    ),
    // Not the prelude's: what the checked program lowers `Secret<Bytes>`'s
    // comparisons and `ct_select` to (`oska-check`'s `lower/secrets.rs`,
    // and `ExprKind::Select` here).
    (
        "Bytes.constant_time_eq",
        Prelude::Runtime("bytes_constant_time_eq"),
    ),
    (
        "Bytes.ct_select",
        Prelude::Checked("bytes_select", Check::BytesSelect),
    ),
    ("Clock.now_millis", Prelude::Runtime("clock_now_millis")),
    (
        "NetConnect.system_name_servers",
        Prelude::Runtime("net_system_name_servers"),
    ),
    ("Random.next_int", Prelude::Runtime("random_next_int")),
    (
        "Random.secure_bytes",
        Prelude::Checked("random_secure_bytes", Check::SecureBytes),
    ),
    ("Budget.limit", Prelude::Runtime("budget_limit")),
    ("Budget.remaining", Prelude::Runtime("budget_remaining")),
    ("Budget.spent", Prelude::Runtime("budget_spent")),
    ("Budget.reserve", Prelude::Runtime("budget_reserve")),
    ("Reservation.debit", Prelude::Runtime("reservation_debit")),
    (
        "Reservation.release",
        Prelude::Runtime("reservation_release"),
    ),
    (
        "OverBudget.to_string",
        Prelude::Runtime("over_budget_to_string"),
    ),
    ("String.to_string", Prelude::Identity),
    ("String.eq", Prelude::Runtime("string_eq")),
    ("Int.to_string", Prelude::Runtime("int_to_string")),
    ("Float.to_string", Prelude::Runtime("float_to_string")),
    ("Float.total_cmp", Prelude::Runtime("float_total_cmp")),
    ("Float.approx_eq", Prelude::Runtime("float_approx_eq")),
    ("String.parse_float", Prelude::Runtime("string_parse_float")),
    (
        "String.parse_float_lenient",
        Prelude::Runtime("string_parse_float_lenient"),
    ),
    (
        "Int.to_float_rounded",
        Prelude::Runtime("int_to_float_rounded"),
    ),
    ("IoError.to_string", Prelude::Field(0)),
    ("String.len", Prelude::Runtime("string_len")),
    ("String.byte_len", Prelude::Runtime("string_byte_len")),
    ("String.contains", Prelude::Runtime("string_contains")),
    ("String.starts_with", Prelude::Runtime("string_starts_with")),
    ("String.ends_with", Prelude::Runtime("string_ends_with")),
    ("String.trim", Prelude::Runtime("string_trim")),
    ("String.trim_start", Prelude::Runtime("string_trim_start")),
    ("String.trim_end", Prelude::Runtime("string_trim_end")),
    (
        "String.to_uppercase",
        Prelude::Runtime("string_to_uppercase"),
    ),
    ("String.find", Prelude::Runtime("string_find")),
    ("String.split_once", Prelude::Runtime("string_split_once")),
    (
        "String.strip_prefix",
        Prelude::Runtime("string_strip_prefix"),
    ),
    (
        "String.strip_suffix",
        Prelude::Runtime("string_strip_suffix"),
    ),
    ("String.is_empty", Prelude::Runtime("string_is_empty")),
    (
        "String.to_lowercase",
        Prelude::Runtime("string_to_lowercase"),
    ),
    ("String.replace", Prelude::Runtime("string_replace")),
    (
        "String.slice",
        Prelude::Checked("string_slice", Check::Slice),
    ),
    (
        "String.repeat",
        Prelude::Checked("string_repeat", Check::Repeat),
    ),
    ("String.assume_trusted", Prelude::Identity),
    ("String.parse_int", Prelude::Runtime("string_parse_int")),
    ("String.split", Prelude::Runtime("string_split")),
    ("String.lines", Prelude::Runtime("string_lines")),
    ("String.chars", Prelude::Runtime("string_chars")),
    ("String.join", Prelude::Runtime("string_join")),
    ("Path.new", Prelude::Runtime("path_new")),
    ("Path.from", Prelude::Runtime("path_new")),
    ("Path.eq", Prelude::Runtime("path_eq")),
    // `std.html`'s `text`, `TrustedHtml.new` and `Html.concat` build a
    // new object: `lower/html.rs`.
    ("Html.raw", Prelude::Identity),
    ("Html.render", Prelude::Field(0)),
    // Attenuation that only narrows the access: the type says which
    // operations a capability has, so the value stays the same.
    ("Fs.read_only", Prelude::Identity),
    ("Fs.write_only", Prelude::Identity),
    ("Dir.read_only", Prelude::Identity),
    ("Dir.write_only", Prelude::Identity),
    ("Fs.restrict_to", Prelude::Runtime("fs_restrict_to")),
    ("FsRead.restrict_to", Prelude::Runtime("fs_restrict_to")),
    ("FsWrite.restrict_to", Prelude::Runtime("fs_restrict_to")),
    (
        "FsRead.restrict_to_file",
        Prelude::Runtime("fs_restrict_to_file"),
    ),
    (
        "FsWrite.restrict_to_file",
        Prelude::Runtime("fs_restrict_to_file"),
    ),
    ("Dir.restrict_to", Prelude::Runtime("dir_restrict_to")),
    ("DirRead.restrict_to", Prelude::Runtime("dir_restrict_to")),
    ("DirWrite.restrict_to", Prelude::Runtime("dir_restrict_to")),
    (
        "DirRead.restrict_to_file",
        Prelude::Runtime("dir_restrict_to_file"),
    ),
    (
        "DirWrite.restrict_to_file",
        Prelude::Runtime("dir_restrict_to_file"),
    ),
    (
        "DirRead.read_to_string",
        Prelude::Runtime("dir_read_to_string"),
    ),
    ("DirRead.read_bytes", Prelude::Runtime("dir_read_bytes")),
    ("DirRead.list_dir", Prelude::Runtime("dir_list_dir")),
    ("DirRead.exists", Prelude::Runtime("dir_exists")),
    ("DirRead.metadata", Prelude::Runtime("dir_metadata")),
    ("DirWrite.append", Prelude::Runtime("dir_append")),
    ("DirWrite.remove_file", Prelude::Runtime("dir_remove_file")),
    ("DirWrite.remove_dir", Prelude::Runtime("dir_remove_dir")),
    ("DirWrite.rename", Prelude::Runtime("dir_rename")),
    ("FileRead.metadata", Prelude::Runtime("file_metadata")),
    ("FileWrite.append", Prelude::Runtime("file_append")),
    ("DirWrite.write", Prelude::Runtime("dir_write")),
    ("DirWrite.write_bytes", Prelude::Runtime("dir_write_bytes")),
    ("DirWrite.create_dir", Prelude::Runtime("dir_create_dir")),
    (
        "FileRead.read_to_string",
        Prelude::Runtime("file_read_to_string"),
    ),
    ("FileRead.read_bytes", Prelude::Runtime("file_read_bytes")),
    ("FileWrite.write", Prelude::Runtime("file_write")),
    (
        "FileWrite.write_bytes",
        Prelude::Runtime("file_write_bytes"),
    ),
    ("TestContext.printed", Prelude::Runtime("test_printed")),
    (
        "TestContext.answer_approval",
        Prelude::Runtime("test_answer_approval"),
    ),
    (
        "TestContext.approvals_asked",
        Prelude::Runtime("test_approvals_asked"),
    ),
    ("Approver.terminal", Prelude::Runtime("approver_terminal")),
    ("Approver.confirm", Prelude::Confirm),
    ("Approval.summary", Prelude::Field(0)),
    ("Approval.details", Prelude::Field(1)),
    ("Approval.matches", Prelude::Runtime("approval_matches")),
    ("Approval.consume", Prelude::Runtime("approval_consume")),
    (
        "ApprovalError.to_string",
        Prelude::Runtime("approval_error_to_string"),
    ),
    // The network (ADR 0025). `Net`, `NetConnect` and `NetListen` hold
    // nothing, so narrowing one to the other is the identity.
    ("Net.connect_only", Prelude::Identity),
    ("Net.listen_only", Prelude::Identity),
    ("Net.restrict_to", Prelude::Runtime("net_restrict_to")),
    (
        "NetConnect.restrict_to",
        Prelude::Runtime("net_restrict_to"),
    ),
    (
        "NetListen.restrict_to_loopback",
        Prelude::Runtime("net_restrict_to_loopback"),
    ),
    ("HostConnect.connect", Prelude::Runtime("host_connect")),
    (
        "HostConnect.exchange_datagram",
        Prelude::Runtime("host_exchange_datagram"),
    ),
    ("HostConnect.name", Prelude::Runtime("host_name")),
    ("HostConnect.port", Prelude::Runtime("host_port")),
    ("LoopbackListen.accept", Prelude::Runtime("loopback_accept")),
    ("LoopbackListen.port", Prelude::Runtime("loopback_port")),
    ("Connection.send", Prelude::Runtime("connection_send")),
    (
        "Connection.send_bytes",
        Prelude::Runtime("connection_send_bytes"),
    ),
    ("Connection.receive", Prelude::Runtime("connection_receive")),
    (
        "Connection.receive_bytes",
        Prelude::Runtime("connection_receive_bytes"),
    ),
    ("Connection.host", Prelude::Runtime("connection_host")),
    ("Connection.close", Prelude::Runtime("connection_close")),
    ("Bytes.new", Prelude::Runtime("bytes_new")),
    (
        "Bytes.zeroed",
        Prelude::Checked("bytes_zeroed", Check::Zeroed),
    ),
    ("Bytes.from_hex", Prelude::Runtime("bytes_from_hex")),
    ("Bytes.len", Prelude::Runtime("bytes_len")),
    ("Bytes.get", Prelude::Runtime("bytes_get")),
    (
        "Bytes.at",
        Prelude::ByteRead {
            width: 1,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.slice",
        Prelude::Checked("bytes_slice", Check::BytesSlice),
    ),
    (
        "Bytes.read_u16_be",
        Prelude::ByteRead {
            width: 2,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.read_u16_le",
        Prelude::ByteRead {
            width: 2,
            big: false,
            secret: false,
        },
    ),
    (
        "Bytes.read_u32_be",
        Prelude::ByteRead {
            width: 4,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.read_u32_le",
        Prelude::ByteRead {
            width: 4,
            big: false,
            secret: false,
        },
    ),
    (
        "Bytes.read_u64_be",
        Prelude::ByteRead {
            width: 8,
            big: true,
            secret: false,
        },
    ),
    (
        "Bytes.read_u64_le",
        Prelude::ByteRead {
            width: 8,
            big: false,
            secret: false,
        },
    ),
    ("Bytes.to_hex", Prelude::Runtime("bytes_to_hex")),
    ("Bytes.to_text", Prelude::Runtime("bytes_to_text")),
    ("Bytes.eq", Prelude::Runtime("bytes_eq")),
    ("Bytes.hash", Prelude::Runtime("bytes_hash")),
    ("Int.cmp", Prelude::Runtime("int_cmp")),
    ("I8.cmp", Prelude::Runtime("int_cmp")),
    ("I16.cmp", Prelude::Runtime("int_cmp")),
    ("I32.cmp", Prelude::Runtime("int_cmp")),
    ("I64.cmp", Prelude::Runtime("int_cmp")),
    ("U8.cmp", Prelude::Runtime("int_cmp")),
    ("U16.cmp", Prelude::Runtime("int_cmp")),
    ("U32.cmp", Prelude::Runtime("int_cmp")),
    ("U64.cmp", Prelude::Runtime("u64_cmp")),
    ("Bool.cmp", Prelude::Runtime("int_cmp")),
    ("String.cmp", Prelude::Runtime("string_cmp")),
    ("Int.hash", Prelude::Runtime("int_hash")),
    ("I8.hash", Prelude::Runtime("int_hash")),
    ("I16.hash", Prelude::Runtime("int_hash")),
    ("I32.hash", Prelude::Runtime("int_hash")),
    ("I64.hash", Prelude::Runtime("int_hash")),
    ("U8.hash", Prelude::Runtime("int_hash")),
    ("U16.hash", Prelude::Runtime("int_hash")),
    ("U32.hash", Prelude::Runtime("int_hash")),
    ("U64.hash", Prelude::Runtime("int_hash")),
    ("Bool.hash", Prelude::Runtime("int_hash")),
    ("String.hash", Prelude::Runtime("string_hash")),
    ("Path.hash", Prelude::Runtime("path_hash")),
    ("String.to_bytes", Prelude::Runtime("string_to_bytes")),
    ("Bytes.assume_trusted", Prelude::Identity),
    (
        "Bytes.push",
        Prelude::Mutating("bytes_push", Check::BytesGrow(1)),
    ),
    (
        "Bytes.set",
        Prelude::Mutating("bytes_set", Check::BytesIndex),
    ),
    (
        "Bytes.append",
        Prelude::Mutating("bytes_append", Check::BytesAppend),
    ),
    (
        "Bytes.push_u16_be",
        Prelude::Mutating("bytes_push_u16_be", Check::BytesGrow(2)),
    ),
    (
        "Bytes.push_u16_le",
        Prelude::Mutating("bytes_push_u16_le", Check::BytesGrow(2)),
    ),
    (
        "Bytes.push_u32_be",
        Prelude::Mutating("bytes_push_u32_be", Check::BytesGrow(4)),
    ),
    (
        "Bytes.push_u32_le",
        Prelude::Mutating("bytes_push_u32_le", Check::BytesGrow(4)),
    ),
    (
        "Bytes.push_u64_be",
        Prelude::Mutating("bytes_push_u64_be", Check::BytesGrow(8)),
    ),
    (
        "Bytes.push_u64_le",
        Prelude::Mutating("bytes_push_u64_le", Check::BytesGrow(8)),
    ),
];

/// `name` without the module a `std` function is called by when the
/// program declares the same name itself (`html.text` for `text`).
pub fn unqualified(name: &str) -> &str {
    oska_check::builtins::std_modules()
        .filter_map(|path| path.rsplit('.').next())
        .find_map(|module| name.strip_prefix(module)?.strip_prefix('.'))
        .unwrap_or(name)
}

/// The implementation of the prelude intrinsic `name`, if there is one.
pub fn prelude(name: &str) -> Option<Prelude> {
    let name = unqualified(name);
    if let Some(method) = name.strip_prefix(SECRET_BYTES) {
        return match prelude(&format!("Bytes.{method}")) {
            Some(Prelude::ByteRead { width, big, .. }) => Some(Prelude::ByteRead {
                width,
                big,
                secret: true,
            }),
            _ => None,
        };
    }
    PRELUDE.iter().find(|(n, _)| *n == name).map(|(_, p)| *p)
}

/// What the lowering prefixes a read of `Secret<Bytes>` with, in place of
/// `Bytes.`, once the checked program's call of it (such as `Secret.at`)
/// is the public read of the bytes inside (`SecretBytes.at` for
/// `Bytes.at`): a [`Prelude::ByteRead`] that is `secret`.
pub const SECRET_BYTES: &str = "SecretBytes.";
