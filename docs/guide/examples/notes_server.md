# Example: an MCP server

A program that is an MCP server for any MCP client, in `examples/notes_server.osk`. It lists and reads notes and saves drafts, and each tool reaches only the directory it needs.

`mcp.serve(stdin, stdout, info, tools)` reads one JSON-RPC message per line from standard input and writes one reply per line to standard output, until standard input closes. It speaks both protocol eras, 2026-07-28 (every request carries its own metadata, and `server/discover` says what the server is) and 2025-11-25 (the `initialize` handshake), and it checks the tools before it reads anything: a duplicate name, a name outside ASCII letters, digits, `_` and `-` (1 to 128 characters), or a schema that is not an object is an `mcp.McpError`.

```oska file=notes_server.osk
// An MCP server in Oska: three tools over standard input and output, for
// any MCP client (Claude Code, an IDE, `npx @modelcontextprotocol/inspector`).
//
// Each tool reaches only what its arm of `call` uses, and `oskac check`
// says so, per tool: `list_notes` and `read_note` reach `notes`, a
// read-only directory; `save_note` reaches `drafts`, a different
// directory it can only write. Nobody writes the tools' hints (read-only,
// destructive): `mcp.serve` gives `list_notes` and `read_note` the
// read-only hint, and `save_note` the destructive one, because that is
// what the checker computed they reach.
//
// A path a client sends is untrusted. It reaches `Path.from` only, and
// the directory capability refuses one that leaves its root, so
// `../secret.txt` reads nothing: the tool answers with an error the model
// can read.
use std.json
use std.mcp

cap struct Notes {
    notes: DirRead,
    drafts: DirWrite,
}

impl mcp.Toolset for Notes {
    fn specs(self) -> List<mcp.ToolSpec> {
        let path = json.Schema.string().describe("A file name under the notes directory.")
        [
            mcp.ToolSpec.new("list_notes", "List the names of the notes.", json.Schema.object([])),
            mcp.ToolSpec.new(
                "read_note",
                "Read one note.",
                json.Schema.object([json.Field.required("path", path)]),
            ).with_title("Read a note"),
            mcp.ToolSpec.new(
                "save_note",
                "Save a draft, replacing a draft of the same name.",
                json.Schema.object([
                    json.Field.required("path", path),
                    json.Field.required("text", json.Schema.string().describe("What the draft says.")),
                ]),
            ),
        ]
    }

    fn call(self, name: String, args: json.Value) -> mcp.ToolResult {
        match name {
            "list_notes" => list(self.notes)
            "read_note" => read(self.notes, args)
            "save_note" => save(self.drafts, args)
            _ => mcp.ToolResult.error("no such tool")
        }
    }
}

fn list(notes: DirRead) -> mcp.ToolResult {
    match notes.list_dir(Path.new(".")) {
        Ok(names) => {
            var values: List<json.Value> = []
            for name in names {
                values.push(json.Value.Text(name))
            }
            mcp.ToolResult.json(json.Value.List(values))
        }
        Err(_) => mcp.ToolResult.error("cannot list the notes")
    }
}

fn read(notes: DirRead, args: json.Value) -> mcp.ToolResult {
    let path = args.text("path").unwrap_or("")
    match notes.read_to_string(Path.from(path)) {
        Ok(text) => mcp.ToolResult.text(text)
        Err(_) => mcp.ToolResult.error("cannot read the note {path}: it is not a note")
    }
}

fn save(drafts: DirWrite, args: json.Value) -> mcp.ToolResult {
    let path = args.text("path").unwrap_or("")
    let text = args.text("text").unwrap_or("")
    match drafts.write(Path.from(path), text) {
        Ok(()) => mcp.ToolResult.text("saved the draft {path}")
        Err(_) => mcp.ToolResult.error("cannot save the draft {path}")
    }
}

fn main(fs: Fs, stdin: Stdin, stdout: Stdout) -> Result<(), mcp.McpError> {
    let tools = Notes(
        notes: fs.read_only().restrict_to("notes"),
        drafts: fs.write_only().restrict_to("drafts"),
    )
    let info = mcp.ServerInfo.new("notes", "1.0.0").with_instructions("Read the notes, and save drafts.")
    mcp.serve(stdin, stdout, info, tools)
}

// The tests drive the server with `mcp.handle` and recorded lines: no
// process, no standard input. `t.fs` is an in-memory filesystem.
const META: String = "\"_meta\":\{\"io.modelcontextprotocol/protocolVersion\":\"2026-07-28\",\"io.modelcontextprotocol/clientCapabilities\":\{\}\}"

fn server(fs: Fs) -> Notes {
    Notes(notes: fs.read_only().restrict_to("notes"), drafts: fs.write_only().restrict_to("drafts"))
}

// The reply to the request `method` with `params`, as a client reads it.
fn reply(tools: Notes, method: String, params: String) -> Untrusted<String> {
    let line = "\{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"{method}\",\"params\":\{{params}{META}\}\}"
    let session = mcp.Session.new(mcp.ServerInfo.new("notes", "1.0.0"))
    mcp.handle(session, tools, line).reply.unwrap_or("(no reply)")
}

test "the tool list gives each tool the hints its authority computes" {
    let list = reply(server(t.fs), "tools/list", "")
    let read_only = "\"annotations\":\{\"readOnlyHint\":true,\"destructiveHint\":false,\"openWorldHint\":false\}"
    let destructive = "\"annotations\":\{\"readOnlyHint\":false,\"destructiveHint\":true,\"openWorldHint\":false\}"
    t.assert(list.contains("\"name\":\"read_note\""))
    t.assert(list.contains(read_only))
    t.assert(list.contains(destructive))
}

test "a path out of the notes directory is a tool error, not a read" {
    let call = "\"name\":\"read_note\",\"arguments\":\{\"path\":\"../secret.txt\"\},"
    let answer = reply(server(t.fs), "tools/call", call)
    t.assert(answer.contains("\"isError\":true"))
    t.assert(answer.contains("it is not a note"))
}

test "arguments that break the schema are an error the client can correct" {
    let call = "\"name\":\"read_note\",\"arguments\":\{\},"
    let answer = reply(server(t.fs), "tools/call", call)
    t.assert(answer.contains("\"isError\":true"))
    t.assert(answer.contains("$.path"))
}

test "an unknown tool is a protocol error" {
    let answer = reply(server(t.fs), "tools/call", "\"name\":\"nope\",")
    t.assert(answer.contains("\"code\":-32602"))
}
```

## What each tool can do

```console cmd=check
fn <Notes as std.mcp.Toolset>.specs: DirRead, DirWrite (self)
fn <Notes as std.mcp.Toolset>.call: DirRead, DirWrite (self)
  tool "list_notes": DirRead (self.notes)
  tool "read_note": DirRead (self.notes)
  tool "save_note": DirWrite (self.drafts)
  default arm: pure
fn list: DirRead (notes)
fn read: DirRead (notes)
fn save: DirWrite (drafts)
fn main: Fs (fs), Stdin (stdin), Stdout (stdout)
  notes_server.osk:87:16: Fs -> FsRead: read_only (bytes 3262..3276)
  notes_server.osk:87:16: FsRead -> DirRead: restrict_to "notes" (bytes 3262..3297)
  notes_server.osk:88:17: Fs -> FsWrite: write_only (bytes 3315..3330)
  notes_server.osk:88:17: FsWrite -> DirWrite: restrict_to "drafts" (bytes 3315..3352)
fn server: Fs (fs)
  notes_server.osk:99:18: Fs -> FsRead: read_only (bytes 3843..3857)
  notes_server.osk:99:18: FsRead -> DirRead: restrict_to "notes" (bytes 3843..3878)
  notes_server.osk:99:63: Fs -> FsWrite: write_only (bytes 3888..3903)
  notes_server.osk:99:63: FsWrite -> DirWrite: restrict_to "drafts" (bytes 3888..3925)
fn reply: DirRead, DirWrite (tools)
```

Read the report as a reviewer would:

- `list_notes` and `read_note` reach `self.notes`, a `DirRead`: they cannot write. `save_note` reaches `self.drafts`, a `DirWrite` on a different directory: it cannot read, and it cannot touch the notes. The default arm reaches nothing.
- `main` holds the whole `Fs` and narrows it in two attenuations before the toolset exists. The server loop, `mcp.serve`, gets only standard input and output.
- No tool prints to standard output. A tool that did would be flagged in the report as writing to the protocol stream, because one stray line would corrupt the framing.

## Hints no one writes

`tools/list` gives every tool `annotations`: `readOnlyHint`, `destructiveHint` and `openWorldHint`. `mcp.serve` computes them from what the checker found each tool reaches, so a program cannot claim a hint its code does not earn. For this server, `list_notes` and `read_note` are read-only, and `save_note` is destructive, because it reaches a write capability. A tool is read-only only when it reaches nothing that writes, uses the network, starts a process or spends a budget; it is destructive when it reaches a write capability or starts a process (append and overwrite are not told apart); it is open-world when it reaches the network. A tool that reaches a capability through a type parameter could reach any, and gets all three. A toolset whose `call` is not a `match` on the tool's name gives every tool the whole toolset's authority. `idempotentHint` is never sent. Clients must treat annotations as untrusted anyway; deriving them makes them honest, not authoritative.

## What a client sees

A modern client may start with `server/discover`, and then sends `tools/list` and `tools/call` with the protocol version and its capabilities in each request's `_meta`:

```text
> {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"read_note","arguments":{"path":"../secret.txt"},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}
< {"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","content":[{"type":"text","text":"cannot read the note ../secret.txt: it is not a note"}],"isError":true,"_meta":{"io.modelcontextprotocol/serverInfo":{"name":"notes","version":"1.0.0"}}}}
```

A path that leaves the notes directory reads nothing: the model-chosen path reaches only `Path.from`, and the directory capability refuses it. The tool answers with an error the model can read, as a tool execution error (`isError`). Arguments that break a tool's input schema are the same kind of error, naming the failing field and what was expected. A call to a tool the server does not have is a JSON-RPC error (`-32602`), which the server answers itself: `Toolset.call` only ever gets a name from its own specs.

The error codes are `-32700` for text that is not JSON (with `id` null), `-32600` for a message that is not a request or is longer than `mcp.MAX_LINE`, `-32601` for an unknown method, `-32602` for malformed parameters, an unknown tool or a request without its `_meta`, and `-32022` for a protocol version the server does not take, with `data.supported` and `data.requested`. A notification never gets a reply. A call runs to completion before the next line is read, so `notifications/cancelled` is accepted and ignored: the reply has been sent already.

To connect a client, give it the command that starts the program, such as `oskac run examples/notes_server.osk` in a directory that has `notes` and `drafts`, or the executable `oskac build` makes.

## Test it without a process

`mcp.handle(session, tools, line)` is what `serve` runs for each line, so the example's `test` blocks drive the server with recorded lines and no process:

```console cmd=test
test the tool list gives each tool the hints its authority computes ... ok
test a path out of the notes directory is a tool error, not a read ... ok
test arguments that break the schema are an error the client can correct ... ok
test an unknown tool is a protocol error ... ok

4 tests: 4 passed, 0 failed, 0 skipped
```

The toolchain's own tests run this example as a process on both backends against transcripts, and compare standard output byte for byte.

Back to the [capabilities example](capabilities.md), or on to [capabilities and authority](../capabilities.md).
