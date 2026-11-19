import Lean.Data.Json
import OskaFormal.Core.Eval

/-!
# Oska Core: reading core IR

The Lean reference reads a program of Oska Core as core IR version 4, the
JSON form `crates/oska-check/src/core_ir.rs` exports, into the syntax of
`Syntax.lean`, and runs its `main` with `run` (`Eval.lean`).

The reader refuses anything that is not core IR version 4, naming what is
wrong and where: a version other than 4 (versions 1 to 3 included), an
unknown `kind`, operator, type or built-in, a field missing, unknown
or of the wrong JSON type, an `Int` literal outside `-2^63 ..= 2^63 - 1`,
two functions or two types of one name, a type named `Int`, `Bool` or
`()` or of a built-in type (`String`, `Path`, a capability), a field of a
struct or variant that is a capability or a function, a closure that
captures a slot at or above its height, two variants of one name in an
enum, a call of a function the program does not have or of a built-in,
with a different number of arguments than it has parameters, and a
constructor (built or matched) of a struct or variant the program does
not declare, or with a different number of fields. `entry` then refuses a
program whose `main` is missing, takes a parameter that is not a
capability the runtime grants (`Stdout`, `Fs`, `FsRead`, `FsWrite`), or
does not return `()`. The reader does not type the program: the checker typed it before
the export, and a run that gets stuck says so. The checks at the end of
this file run the reader on good and bad core IR.

The reader is a parser, not part of the semantics, so it is `partial`
and carries no proof.
-/

namespace OskaFormal.Core.CoreIR

open Lean (Json)

/-- A reader's result: the value, or what is wrong with the core IR. -/
abbrev Read := Except String

/-- The fields of the object `j` at `path`, refusing any field outside
`allowed`. -/
def fields (path : String) (j : Json) (allowed : List String) : Read (List String) :=
  match j with
  | .obj kvs =>
    match kvs.keys.find? (fun k => !allowed.contains k) with
    | some k => .error s!"{path}: unknown field `{k}`"
    | none => .ok kvs.keys
  | _ => .error s!"{path}: expected an object"

/-- The field `name` of the object `j` at `path`. -/
def field (path : String) (j : Json) (name : String) : Read Json :=
  match j.getObjVal? name with
  | .ok v => .ok v
  | .error _ => .error s!"{path}: missing field `{name}`"

def str (path : String) : Json → Read String
  | .str s => .ok s
  | _ => .error s!"{path}: expected a string"

def bool (path : String) : Json → Read Bool
  | .bool b => .ok b
  | _ => .error s!"{path}: expected true or false"

def arr (path : String) : Json → Read (List Json)
  | .arr items => .ok items.toList
  | _ => .error s!"{path}: expected an array"

def int (path : String) (j : Json) : Read Int :=
  match j with
  | .num _ =>
    match j.getInt? with
    | .ok n => .ok n
    | .error _ => .error s!"{path}: expected an integer"
  | _ => .error s!"{path}: expected an integer"

def nat (path : String) (j : Json) : Read Nat := do
  let n ← int path j
  if n < 0 then .error s!"{path}: expected a slot, 0 or more" else .ok n.toNat

/-- A capability kind, by the name of its type. -/
def capKind : String → Option CapKind
  | "Stdout" => some .stdout
  | "Fs" => some .fs | "FsRead" => some .fsRead | "FsWrite" => some .fsWrite
  | "Dir" => some .dir | "DirRead" => some .dirRead | "DirWrite" => some .dirWrite
  | _ => none

/-- The names of the built-in types, which no declared type may take. -/
def builtinTypes : List String :=
  ["Int", "Bool", "()", "String", "Path", "Stdout", "Fs", "FsRead", "FsWrite", "Dir", "DirRead",
    "DirWrite"]

/-- A type: `Int`, `Bool`, `()`, `String`, `Path`, a capability, the
`Result` of an effect, one of the struct and enum names `types`, or a
function type, `{"kind": "Fn", "params": [type...], "ret": type}`. -/
partial def ty (types : List String) (path : String) (j : Json) : Read Ty := do
  if let .obj _ := j then
    let _ ← fields path j ["kind", "params", "ret"]
    let kind ← str s!"{path}.kind" (← field path j "kind")
    if kind ≠ "Fn" then throw s!"{path}: unknown type kind `{kind}`"
    let items ← arr s!"{path}.params" (← field path j "params")
    let ps ← (items.zip (List.range items.length)).mapM fun (p, i) =>
      ty types s!"{path}.params[{i}]" p
    return .fn ps (← ty types s!"{path}.ret" (← field path j "ret"))
  match ← str path j with
  | "Int" => .ok .int
  | "Bool" => .ok .bool
  | "()" => .ok .unit
  | "String" => .ok .string
  | "Path" => .ok .path
  | "Result<(), IoError>" => .ok (.result .unit)
  | "Result<String, IoError>" => .ok (.result .string)
  | t =>
    match capKind t with
    | some k => .ok (.cap k)
    | none => if types.contains t then .ok (.named t) else .error s!"{path}: unknown type `{t}`"

/-- A built-in call, by its core IR name, and how many arguments it takes,
its receiver included. -/
def builtin : String → Option (Builtin × Nat)
  | "Stdout.println" => some (.println, 2)
  | "Fs.restrict_to" => some (.restrictTo .fs, 2)
  | "FsRead.restrict_to" => some (.restrictTo .fsRead, 2)
  | "FsWrite.restrict_to" => some (.restrictTo .fsWrite, 2)
  | "Dir.restrict_to" => some (.restrictTo .dir, 2)
  | "DirRead.restrict_to" => some (.restrictTo .dirRead, 2)
  | "DirWrite.restrict_to" => some (.restrictTo .dirWrite, 2)
  | "Fs.read_only" => some (.readOnly .fs, 1)
  | "Fs.write_only" => some (.writeOnly .fs, 1)
  | "Dir.read_only" => some (.readOnly .dir, 1)
  | "Dir.write_only" => some (.writeOnly .dir, 1)
  | "DirRead.read_to_string" => some (.readToString, 2)
  | "DirWrite.write" => some (.write, 3)
  | "DirWrite.create_dir" => some (.createDir, 2)
  | "Path.from" => some (.pathFrom, 1)
  | "Path.new" => some (.pathNew, 1)
  | "Result.is_ok" => some (.isOk, 1)
  | "Result.unwrap_or" => some (.unwrapOr, 2)
  | _ => none

def binOp (path : String) : String → Read BinOp
  | "+" => .ok .add | "-" => .ok .sub | "*" => .ok .mul | "/" => .ok .div | "%" => .ok .rem
  | "<" => .ok .lt | "<=" => .ok .le | ">" => .ok .gt | ">=" => .ok .ge
  | "==" => .ok .eq | "!=" => .ok .ne
  | "&&" => .ok .and | "||" => .ok .or | "implies" => .ok .implies
  | op => .error s!"{path}: unknown operator `{op}`"

/-- What reading a function's body checks names against: the number of
parameters of each function, by name, for each call's arity, and the
struct and enum declarations, for each constructor. -/
structure Ctx where
  arities : List (String × Nat)
  types : List TypeDecl

/-- Read the list `items` at `path` with `read`, each at `path[i]`. -/
def list {α : Type} (path : String) (items : List Json) (read : String → Json → Read α) :
    Read (List α) :=
  (items.zip (List.range items.length)).mapM fun (j, i) => read s!"{path}[{i}]" j

/-- The `variant` field of a constructor, if it has one. -/
def variant (path : String) (j : Json) (keys : List String) : Read (Option String) :=
  if keys.contains "variant" then do some <$> str s!"{path}.variant" (← field path j "variant")
  else pure none

/-- Check that a constructor of `type` and `variant` with `n` fields is one
the program declares. -/
def ctor (ctx : Ctx) (path : String) (type : String) (variant : Option String) (n : Nat) :
    Read Unit := do
  let fts ← match findType ctx.types type, variant with
    | none, _ => .error s!"{path}: `{type}` is not a struct or enum the program declares"
    | some (.struct_ _ fs), none => pure fs
    | some (.struct_ _ _), some v => .error s!"{path}: `{type}` is a struct, with no variant `{v}`"
    | some (.enum_ _ _), none => .error s!"{path}: `{type}` is an enum, and no variant is named"
    | some (.enum_ _ vs), some v =>
      match vs.lookup v with
      | some fs => pure fs
      | none => .error s!"{path}: the enum `{type}` has no variant `{v}`"
  if fts.length ≠ n then
    .error s!"{path}: a constructor of `{type}` with {n} field(s), but it has {fts.length}"

/-- A pattern of core IR. -/
partial def pattern (ctx : Ctx) (path : String) (j : Json) : Read Pattern := do
  let kind ← str s!"{path}.kind" (← field path j "kind")
  match kind with
  | "Wildcard" =>
    let _ ← fields path j ["kind"]
    .ok .wildcard
  | "Bind" =>
    let _ ← fields path j ["kind", "type"]
    .ok (.bind (← ty (ctx.types.map TypeDecl.name) s!"{path}.type" (← field path j "type")))
  | "Unit" =>
    let _ ← fields path j ["kind"]
    .ok .unit
  | "Int" =>
    let _ ← fields path j ["kind", "value"]
    let n ← int s!"{path}.value" (← field path j "value")
    if fits n then .ok (.int n)
    else .error s!"{path}.value: {n} is outside Int, -2^63 ..= 2^63 - 1"
  | "Bool" =>
    let _ ← fields path j ["kind", "value"]
    .ok (.bool (← bool s!"{path}.value" (← field path j "value")))
  | "Construct" =>
    let keys ← fields path j ["kind", "type", "variant", "fields"]
    let type ← str s!"{path}.type" (← field path j "type")
    let v ← variant path j keys
    let ps ← list s!"{path}.fields" (← arr s!"{path}.fields" (← field path j "fields"))
      (pattern ctx)
    ctor ctx path type v ps.length
    .ok (.construct type v ps)
  | k => .error s!"{path}: unknown pattern kind `{k}`"

/-- A parameter of a function or closure. -/
def param (types : List String) (path : String) (j : Json) : Read Param := do
  let _ ← fields path j ["type", "binds"]
  .ok { type := ← ty types s!"{path}.type" (← field path j "type"),
        binds := ← bool s!"{path}.binds" (← field path j "binds") }

mutual
  /-- An expression of core IR. -/
  partial def expr (ctx : Ctx) (path : String) (j : Json) : Read Expr := do
    let kind ← str s!"{path}.kind" (← field path j "kind")
    let at_ (name : String) := s!"{path}.{name}"
    let sub (name : String) : Read Expr := do expr ctx (at_ name) (← field path j name)
    match kind with
    | "Unit" =>
      let _ ← fields path j ["kind"]
      .ok .unit
    | "Int" =>
      let _ ← fields path j ["kind", "value"]
      let n ← int (at_ "value") (← field path j "value")
      if fits n then .ok (.int n)
      else .error s!"{at_ "value"}: {n} is outside Int, -2^63 ..= 2^63 - 1"
    | "Bool" =>
      let _ ← fields path j ["kind", "value"]
      .ok (.bool (← bool (at_ "value") (← field path j "value")))
    | "Str" =>
      let _ ← fields path j ["kind", "value"]
      .ok (.str (← str (at_ "value") (← field path j "value")))
    | "Builtin" =>
      let _ ← fields path j ["kind", "name", "args"]
      let name ← str (at_ "name") (← field path j "name")
      let args ← list (at_ "args") (← arr (at_ "args") (← field path j "args")) (expr ctx)
      match builtin name with
      | none => .error s!"{path}: unknown built-in `{name}`"
      | some (op, n) =>
        if args.length = n then .ok (.builtin op args)
        else .error s!"{path}: a call of the built-in `{name}` with {args.length} argument(s), but it has {n} parameter(s)"
    | "Local" =>
      let _ ← fields path j ["kind", "slot"]
      .ok (.local_ (← nat (at_ "slot") (← field path j "slot")))
    | "Neg" =>
      let _ ← fields path j ["kind", "operand"]
      .ok (.neg (← sub "operand"))
    | "Not" =>
      let _ ← fields path j ["kind", "operand"]
      .ok (.not (← sub "operand"))
    | "Binary" =>
      let _ ← fields path j ["kind", "op", "lhs", "rhs"]
      let op ← binOp (at_ "op") (← str (at_ "op") (← field path j "op"))
      .ok (.binary op (← sub "lhs") (← sub "rhs"))
    | "Call" =>
      let _ ← fields path j ["kind", "function", "args"]
      let name ← str (at_ "function") (← field path j "function")
      let args ← list (at_ "args") (← arr (at_ "args") (← field path j "args")) (expr ctx)
      match ctx.arities.lookup name with
      | none => .error s!"{path}: a call of `{name}`, which the program does not define"
      | some n =>
        if args.length = n then .ok (.call name args)
        else .error s!"{path}: a call of `{name}` with {args.length} argument(s), but it has {n} parameter(s)"
    | "If" =>
      let keys ← fields path j ["kind", "cond", "then", "else"]
      let el ← if keys.contains "else" then some <$> sub "else" else pure none
      .ok (.if_ (← sub "cond") (← sub "then") el)
    | "Block" =>
      let keys ← fields path j ["kind", "stmts", "value"]
      let ss ← list (at_ "stmts") (← arr (at_ "stmts") (← field path j "stmts")) (stmt ctx)
      let val ← if keys.contains "value" then some <$> sub "value" else pure none
      .ok (.block ss val)
    | "Construct" =>
      let keys ← fields path j ["kind", "type", "variant", "fields"]
      let type ← str (at_ "type") (← field path j "type")
      let v ← variant path j keys
      let fs ← list (at_ "fields") (← arr (at_ "fields") (← field path j "fields")) (expr ctx)
      ctor ctx path type v fs.length
      .ok (.construct type v fs)
    | "Field" =>
      let _ ← fields path j ["kind", "base", "index"]
      let base ← sub "base"
      .ok (.field base (← nat (at_ "index") (← field path j "index")))
    | "Match" =>
      let _ ← fields path j ["kind", "scrutinee", "arms"]
      let sc ← sub "scrutinee"
      let arms ← list (at_ "arms") (← arr (at_ "arms") (← field path j "arms")) (arm ctx)
      .ok (.match_ sc arms)
    | "Closure" =>
      let _ ← fields path j ["kind", "params", "captures", "height", "ret", "body"]
      let types := ctx.types.map TypeDecl.name
      let ps ← list (at_ "params") (← arr (at_ "params") (← field path j "params")) (param types)
      let height ← nat (at_ "height") (← field path j "height")
      let cs ← list (at_ "captures") (← arr (at_ "captures") (← field path j "captures")) nat
      if let some c := cs.find? (· ≥ height) then
        throw s!"{at_ "captures"}: slot {c} is not below the closure's height {height}"
      let ret ← ty types (at_ "ret") (← field path j "ret")
      .ok (.closure ps height cs ret (← sub "body"))
    | "CallValue" =>
      let _ ← fields path j ["kind", "callee", "args"]
      let callee ← sub "callee"
      let args ← list (at_ "args") (← arr (at_ "args") (← field path j "args")) (expr ctx)
      .ok (.callValue callee args)
    | k => .error s!"{path}: unknown expression kind `{k}`"

  /-- A statement of core IR. -/
  partial def stmt (ctx : Ctx) (path : String) (j : Json) : Read Stmt := do
    let kind ← str s!"{path}.kind" (← field path j "kind")
    match kind with
    | "Let" =>
      let _ ← fields path j ["kind", "binds", "type", "value"]
      let binds ← bool s!"{path}.binds" (← field path j "binds")
      let t ← ty (ctx.types.map TypeDecl.name) s!"{path}.type" (← field path j "type")
      .ok (.let_ binds t (← expr ctx s!"{path}.value" (← field path j "value")))
    | "Expr" =>
      let _ ← fields path j ["kind", "value"]
      .ok (.expr (← expr ctx s!"{path}.value" (← field path j "value")))
    | k => .error s!"{path}: unknown statement kind `{k}`"

  /-- A `match` arm of core IR. -/
  partial def arm (ctx : Ctx) (path : String) (j : Json) : Read Arm := do
    let keys ← fields path j ["pattern", "guard", "body"]
    let p ← pattern ctx s!"{path}.pattern" (← field path j "pattern")
    let g ← if keys.contains "guard" then
        some <$> expr ctx s!"{path}.guard" (← field path j "guard")
      else pure none
    .ok (.mk p g (← expr ctx s!"{path}.body" (← field path j "body")))
end

/-- The first name that `names` holds twice, if any. -/
def duplicate (names : List String) : Option String :=
  names.find? (fun n => (names.filter (· == n)).length > 1)

/-- A struct or enum declaration of core IR, whose field types may name
any of `types`. -/
def typeDecl (types : List String) (path : String) (j : Json) : Read TypeDecl := do
  let kind ← str s!"{path}.kind" (← field path j "kind")
  let tys (path : String) (j : Json) : Read (List Ty) := do
    let ts ← list path (← arr path j) (ty types)
    if ts.any (·.capKind?.isSome) then
      throw s!"{path}: a field is a capability; no struct or enum of Oska Core holds one"
    if ts.any (·.isFn) then
      throw s!"{path}: a field is a function; no struct or enum of Oska Core holds one"
    pure ts
  match kind with
  | "Struct" =>
    let _ ← fields path j ["kind", "name", "fields"]
    let name ← str s!"{path}.name" (← field path j "name")
    .ok (.struct_ name (← tys s!"{path}.fields" (← field path j "fields")))
  | "Enum" =>
    let _ ← fields path j ["kind", "name", "variants"]
    let name ← str s!"{path}.name" (← field path j "name")
    let vs ← list s!"{path}.variants" (← arr s!"{path}.variants" (← field path j "variants"))
      fun path v => do
        let _ ← fields path v ["name", "fields"]
        let vname ← str s!"{path}.name" (← field path v "name")
        pure (vname, ← tys s!"{path}.fields" (← field path v "fields"))
    if let some dup := duplicate (vs.map (·.1)) then
      throw s!"{path}: two variants of `{name}` are named `{dup}`"
    .ok (.enum_ name vs)
  | k => .error s!"{path}: unknown type declaration kind `{k}`"

/-- The version of core IR this reader reads. -/
def version : Int := 4

/-- A program of Oska Core, read from the text of its core IR. -/
def program (text : String) : Read Program := do
  let j ← match Json.parse text with
    | .ok j => pure j
    | .error e => .error s!"not JSON: {e}"
  let _ ← fields "program" j ["core_ir_version", "types", "functions"]
  let v ← int "core_ir_version" (← field "program" j "core_ir_version")
  if v ≠ version then
    throw s!"core_ir_version {v}: this Lean reference reads core IR version {version} only"
  let ts ← arr "types" (← field "program" j "types")
  -- The type names first, so a field may have a type declared later.
  let typeNames ← list "types" ts fun path t => do str s!"{path}.name" (← field path t "name")
  if let some dup := duplicate typeNames then
    throw s!"types: two types are named `{dup}`"
  if let some builtin := typeNames.find? (builtinTypes.contains ·) then
    throw s!"types: a type is named `{builtin}`, the name of a built-in type"
  let types ← list "types" ts (typeDecl typeNames)
  let fs ← arr "functions" (← field "program" j "functions")
  -- The names and arities first, so a call may reach a later function.
  let heads ← list "functions" fs fun path f => do
    let _ ← fields path f ["name", "params", "ret", "body"]
    let name ← str s!"{path}.name" (← field path f "name")
    let params ← list s!"{path}.params" (← arr s!"{path}.params" (← field path f "params"))
      (param typeNames)
    pure (name, params)
  if let some dup := duplicate (heads.map (·.1)) then
    throw s!"functions: two functions are named `{dup}`"
  let ctx : Ctx := { arities := heads.map fun (n, ps) => (n, ps.length), types }
  let functions ← (fs.zip heads).zip (List.range fs.length) |>.mapM fun ((f, (name, params)), i) => do
    let path := s!"functions[{i}]"
    pure { name, params,
           ret := ← ty typeNames s!"{path}.ret" (← field path f "ret"),
           body := ← expr ctx s!"{path}.body" (← field path f "body") }
  pure { types, functions }

/-- The expression a run of program `P` starts from: a call of its `main`,
which returns `()`, with the capability each parameter's type names,
rooted anywhere, as the runtime grants them (`grants`): the root of
authority. -/
def entry (P : Program) : Read Expr :=
  match P.find? "main" with
  | none => .error "the program has no function `main`"
  | some f =>
    if f.params.any (fun p => !(p.type.capKind?.map CapKind.granted).getD false) then
      .error "`main` takes a parameter that is not a capability the runtime grants"
    else if f.ret ≠ .unit then .error "`main` does not return `()`"
    else .ok (.call "main" (grants f.params))

/-- Whether reading `text` fails with a message that contains `part`. -/
def refuses (text part : String) : Bool :=
  match program text with
  | .error e => (e.splitOn part).length > 1
  | .ok _ => false

/-- Whether `text` reads as a program whose `main` the Lean reference
runs to `outcome`, within 1000 steps. -/
def runsTo (text : String) (outcome : RunResult → Bool) : Bool :=
  match program text >>= fun P => (entry P).map (P, ·) with
  | .ok (P, e) => outcome (run P 1000 {} e).1
  | .error _ => false

/-- Core IR version 4 with these `types` and a `main` with this body. -/
def withMain (types body : String) : String :=
  "{\"core_ir_version\": 4, \"types\": [" ++ types ++ "], \"functions\": [{\"name\": \"main\", " ++
    "\"params\": [], \"ret\": \"()\", \"body\": " ++ body ++ "}]}"

/-- `enum Shape { Dot, Square(Int) }` and `struct Pair { a: Int, b: Bool }`. -/
def shapeTypes : String :=
  "{\"kind\": \"Enum\", \"name\": \"Shape\", \"variants\": [{\"name\": \"Dot\", \"fields\": []}, " ++
    "{\"name\": \"Square\", \"fields\": [\"Int\"]}]}, " ++
    "{\"kind\": \"Struct\", \"name\": \"Pair\", \"fields\": [\"Int\", \"Bool\"]}"

#guard refuses "{\"core_ir_version\": 5, \"types\": [], \"functions\": []}" "core_ir_version 5"
-- Versions 1 to 3 are refused by their version.
#guard refuses "{\"core_ir_version\": 1, \"functions\": []}" "core_ir_version 1"
#guard refuses "{\"core_ir_version\": 2, \"types\": [], \"functions\": []}" "core_ir_version 2"
#guard refuses "{\"core_ir_version\": 3, \"types\": [], \"functions\": []}" "core_ir_version 3"
#guard refuses "{\"core_ir_version\": 4, \"functions\": []}" "missing field `types`"
#guard refuses "{\"core_ir_version\": 4, \"types\": [], \"functions\": []} junk" "not JSON"
#guard refuses "{\"core_ir_version\": 4, \"types\": [], \"functions\": [], \"x\": 1}"
  "unknown field `x`"
#guard refuses (withMain "" "{\"kind\": \"Loop\"}") "unknown expression kind `Loop`"
#guard refuses (withMain "" "{\"kind\": \"Int\", \"value\": 9223372036854775808}") "outside Int"
#guard refuses (withMain "" "{\"kind\": \"Call\", \"function\": \"main\", \"args\": [{\"kind\": \"Unit\"}]}")
  "with 1 argument(s), but it has 0 parameter(s)"
#guard refuses (withMain "" "{\"kind\": \"Call\", \"function\": \"absent\", \"args\": []}")
  "does not define"
#guard refuses (withMain shapeTypes "{\"kind\": \"Construct\", \"type\": \"Point\", \"fields\": []}")
  "`Point` is not a struct or enum the program declares"
#guard refuses (withMain shapeTypes
    "{\"kind\": \"Construct\", \"type\": \"Shape\", \"variant\": \"Circle\", \"fields\": []}")
  "the enum `Shape` has no variant `Circle`"
#guard refuses (withMain shapeTypes "{\"kind\": \"Construct\", \"type\": \"Shape\", \"fields\": []}")
  "`Shape` is an enum, and no variant is named"
#guard refuses (withMain shapeTypes
    "{\"kind\": \"Construct\", \"type\": \"Pair\", \"variant\": \"Dot\", \"fields\": []}")
  "`Pair` is a struct, with no variant `Dot`"
#guard refuses (withMain shapeTypes
    "{\"kind\": \"Construct\", \"type\": \"Pair\", \"fields\": [{\"kind\": \"Int\", \"value\": 1}]}")
  "with 1 field(s), but it has 2"
#guard refuses (withMain shapeTypes ("{\"kind\": \"Match\", \"scrutinee\": {\"kind\": \"Unit\"}, " ++
    "\"arms\": [{\"pattern\": {\"kind\": \"Construct\", \"type\": \"Shape\", \"variant\": \"Square\", " ++
    "\"fields\": []}, \"body\": {\"kind\": \"Unit\"}}]}"))
  "with 0 field(s), but it has 1"
#guard refuses (withMain shapeTypes ("{\"kind\": \"Match\", \"scrutinee\": {\"kind\": \"Unit\"}, " ++
    "\"arms\": [{\"pattern\": {\"kind\": \"Range\"}, \"body\": {\"kind\": \"Unit\"}}]}"))
  "unknown pattern kind `Range`"
#guard refuses (withMain "{\"kind\": \"Struct\", \"name\": \"A\", \"fields\": [\"U8\"]}"
    "{\"kind\": \"Unit\"}")
  "unknown type `U8`"
#guard refuses (withMain ("{\"kind\": \"Struct\", \"name\": \"A\", \"fields\": []}, " ++
    "{\"kind\": \"Struct\", \"name\": \"A\", \"fields\": []}") "{\"kind\": \"Unit\"}")
  "two types are named `A`"
#guard refuses (withMain "{\"kind\": \"Struct\", \"name\": \"Int\", \"fields\": []}"
    "{\"kind\": \"Unit\"}")
  "a type is named `Int`, the name of a built-in type"
#guard refuses (withMain "{\"kind\": \"Enum\", \"name\": \"()\", \"variants\": []}"
    "{\"kind\": \"Unit\"}")
  "a type is named `()`"
#guard refuses (withMain "{\"kind\": \"Struct\", \"name\": \"Path\", \"fields\": []}"
    "{\"kind\": \"Unit\"}")
  "a type is named `Path`"
#guard refuses (withMain "{\"kind\": \"Struct\", \"name\": \"Holder\", \"fields\": [\"Stdout\"]}"
    "{\"kind\": \"Unit\"}")
  "a field is a capability"
#guard refuses (withMain ("{\"kind\": \"Struct\", \"name\": \"Holder\", \"fields\": [" ++
    "{\"kind\": \"Fn\", \"params\": [], \"ret\": \"()\"}]}") "{\"kind\": \"Unit\"}")
  "a field is a function"
#guard refuses (withMain "" ("{\"kind\": \"Closure\", \"params\": [], \"captures\": [0], " ++
    "\"height\": 0, \"ret\": \"()\", \"body\": {\"kind\": \"Unit\"}}"))
  "slot 0 is not below the closure's height 0"
#guard refuses (withMain "" ("{\"kind\": \"Closure\", \"params\": [], \"captures\": [], " ++
    "\"height\": 0, \"ret\": {\"kind\": \"Tuple\"}, \"body\": {\"kind\": \"Unit\"}}"))
  "unknown type kind `Tuple`"
#guard refuses (withMain "" "{\"kind\": \"Builtin\", \"name\": \"Stdin.read_line\", \"args\": []}")
  "unknown built-in `Stdin.read_line`"
#guard refuses (withMain "" ("{\"kind\": \"Builtin\", \"name\": \"Path.from\", \"args\": [" ++
    "{\"kind\": \"Str\", \"value\": \"a\"}, {\"kind\": \"Str\", \"value\": \"b\"}]}"))
  "with 2 argument(s), but it has 1 parameter(s)"
#guard runsTo (withMain "" "{\"kind\": \"Block\", \"stmts\": []}")
  fun | .value .unit => true | _ => false
#guard runsTo
  (withMain "" ("{\"kind\": \"Block\", \"stmts\": [{\"kind\": \"Let\", \"binds\": false, " ++
    "\"type\": \"Int\", \"value\": {\"kind\": \"Binary\", \"op\": \"/\", " ++
    "\"lhs\": {\"kind\": \"Int\", \"value\": 1}, \"rhs\": {\"kind\": \"Int\", \"value\": 0}}}]}"))
  fun | .panic .divisionByZero => true | _ => false
-- `match Shape.Square(7) { Dot => (), Square(n) if n > 5 => 1 / 0 ... }`: the guard holds, so
-- the arm's body runs and divides by zero; a field read picks the second field.
#guard runsTo
  (withMain shapeTypes ("{\"kind\": \"Match\", \"scrutinee\": {\"kind\": \"Construct\", " ++
    "\"type\": \"Shape\", \"variant\": \"Square\", \"fields\": [{\"kind\": \"Int\", \"value\": 7}]}, " ++
    "\"arms\": [{\"pattern\": {\"kind\": \"Construct\", \"type\": \"Shape\", \"variant\": \"Dot\", " ++
    "\"fields\": []}, \"body\": {\"kind\": \"Unit\"}}, {\"pattern\": {\"kind\": \"Construct\", " ++
    "\"type\": \"Shape\", \"variant\": \"Square\", \"fields\": [{\"kind\": \"Bind\", \"type\": \"Int\"}]}, " ++
    "\"guard\": {\"kind\": \"Binary\", \"op\": \">\", \"lhs\": {\"kind\": \"Local\", \"slot\": 0}, " ++
    "\"rhs\": {\"kind\": \"Int\", \"value\": 5}}, \"body\": {\"kind\": \"Block\", \"stmts\": [" ++
    "{\"kind\": \"Let\", \"binds\": false, \"type\": \"Bool\", \"value\": {\"kind\": \"Field\", " ++
    "\"base\": {\"kind\": \"Construct\", \"type\": \"Pair\", \"fields\": [{\"kind\": \"Int\", " ++
    "\"value\": 1}, {\"kind\": \"Bool\", \"value\": true}]}, \"index\": 1}}, {\"kind\": \"Let\", " ++
    "\"binds\": false, \"type\": \"Int\", \"value\": {\"kind\": \"Binary\", \"op\": \"/\", " ++
    "\"lhs\": {\"kind\": \"Local\", \"slot\": 0}, \"rhs\": {\"kind\": \"Int\", \"value\": 0}}}]}}, " ++
    "{\"pattern\": {\"kind\": \"Wildcard\"}, \"body\": {\"kind\": \"Unit\"}}]}"))
  fun | .panic .divisionByZero => true | _ => false
-- `let n = 7; let f = |x: Int| x / (x - n); let _ = f(7)`: the closure captures `n`, slot 0,
-- and the call divides by zero.
#guard runsTo
  (withMain "" ("{\"kind\": \"Block\", \"stmts\": [{\"kind\": \"Let\", \"binds\": true, " ++
    "\"type\": \"Int\", \"value\": {\"kind\": \"Int\", \"value\": 7}}, {\"kind\": \"Let\", " ++
    "\"binds\": true, \"type\": {\"kind\": \"Fn\", \"params\": [\"Int\"], \"ret\": \"Int\"}, " ++
    "\"value\": {\"kind\": \"Closure\", \"params\": [{\"type\": \"Int\", \"binds\": true}], " ++
    "\"captures\": [0], \"height\": 1, \"ret\": \"Int\", \"body\": {\"kind\": \"Binary\", " ++
    "\"op\": \"/\", \"lhs\": {\"kind\": \"Local\", \"slot\": 1}, \"rhs\": {\"kind\": " ++
    "\"Binary\", \"op\": \"-\", \"lhs\": {\"kind\": \"Local\", \"slot\": 1}, \"rhs\": " ++
    "{\"kind\": \"Local\", \"slot\": 0}}}}}, {\"kind\": \"Let\", \"binds\": false, " ++
    "\"type\": \"Int\", \"value\": {\"kind\": \"CallValue\", \"callee\": {\"kind\": " ++
    "\"Local\", \"slot\": 1}, \"args\": [{\"kind\": \"Int\", \"value\": 7}]}}]}"))
  fun | .panic .divisionByZero => true | _ => false

end OskaFormal.Core.CoreIR
