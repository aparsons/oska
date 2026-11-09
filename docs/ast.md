# Oska AST export

`oskac parse <file.osk> --format=json` writes the syntax tree as JSON. Agents and tools can build on this format.

## Envelope

```json
{
  "ast_version": 1,
  "file": "examples/hello.osk",
  "items": [ ... ]
}
```

## Stability

- Within a major `ast_version`, changes are **additive only**. New node kinds and new fields can appear. Existing fields are never renamed, removed, or given a new meaning.
- Consumers must ignore fields and node kinds they don't recognize.
- Breaking changes increase `ast_version`. The previous version stays available through `--ast-version=N` for at least one release.

## Nodes

Every node is a JSON object with:

- `"kind"`: the node type, as a string, for example `"FnItem"` or `"CallExpr"`.
- `"span"`: `{"start": <byte>, "end": <byte>}`, a half-open byte range into the source file. These are the same spans that diagnostics use.

## Node schema

To be written alongside the parser (#6). Each node kind will be documented here with its fields.
