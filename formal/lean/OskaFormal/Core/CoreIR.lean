import Lean.Data.Json
import OskaFormal.Core.Eval

/-!
# Oska Core: reading core IR

The Lean reference reads a program of Oska Core as core IR version 1, the
JSON form `crates/oska-check/src/core_ir.rs` exports, into the syntax of
`Syntax.lean`, and runs its `main` with `run` (`Eval.lean`).

The reader refuses anything that is not core IR version 1, naming what is
wrong and where: a version other than 1, an unknown `kind`, operator or
type, a field missing, unknown or of the wrong JSON type, an `Int`
literal outside `-2^63 ..= 2^63 - 1`, two functions of one name, a call
of a function the program does not have or with as many arguments as it
has parameters, and a program whose `main` is missing, takes parameters
or does not return `()`. It does not type the program: the checker typed
it before the export, and a run that gets stuck says so.

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

def ty (path : String) (j : Json) : Read Ty := do
  match ← str path j with
  | "Int" => .ok .int
  | "Bool" => .ok .bool
  | "()" => .ok .unit
  | t => .error s!"{path}: unknown type `{t}`"

def binOp (path : String) : String → Read BinOp
  | "+" => .ok .add | "-" => .ok .sub | "*" => .ok .mul | "/" => .ok .div | "%" => .ok .rem
  | "<" => .ok .lt | "<=" => .ok .le | ">" => .ok .gt | ">=" => .ok .ge
  | "==" => .ok .eq | "!=" => .ok .ne
  | "&&" => .ok .and | "||" => .ok .or | "implies" => .ok .implies
  | op => .error s!"{path}: unknown operator `{op}`"

/-- The number of parameters of each function, by name, for checking each
call's arity. -/
abbrev Arities := List (String × Nat)

/-- Read the list `items` at `path` with `read`, each at `path[i]`. -/
def list {α : Type} (path : String) (items : List Json) (read : String → Json → Read α) :
    Read (List α) :=
  (items.zip (List.range items.length)).mapM fun (j, i) => read s!"{path}[{i}]" j

mutual
  /-- An expression of core IR. -/
  partial def expr (arities : Arities) (path : String) (j : Json) : Read Expr := do
    let kind ← str s!"{path}.kind" (← field path j "kind")
    let at_ (name : String) := s!"{path}.{name}"
    let sub (name : String) : Read Expr := do expr arities (at_ name) (← field path j name)
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
      let args ← list (at_ "args") (← arr (at_ "args") (← field path j "args")) (expr arities)
      match arities.lookup name with
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
      let ss ← list (at_ "stmts") (← arr (at_ "stmts") (← field path j "stmts")) (stmt arities)
      let val ← if keys.contains "value" then some <$> sub "value" else pure none
      .ok (.block ss val)
    | k => .error s!"{path}: unknown expression kind `{k}`"

  /-- A statement of core IR. -/
  partial def stmt (arities : Arities) (path : String) (j : Json) : Read Stmt := do
    let kind ← str s!"{path}.kind" (← field path j "kind")
    match kind with
    | "Let" =>
      let _ ← fields path j ["kind", "binds", "type", "value"]
      let binds ← bool s!"{path}.binds" (← field path j "binds")
      let t ← ty s!"{path}.type" (← field path j "type")
      .ok (.let_ binds t (← expr arities s!"{path}.value" (← field path j "value")))
    | "Expr" =>
      let _ ← fields path j ["kind", "value"]
      .ok (.expr (← expr arities s!"{path}.value" (← field path j "value")))
    | k => .error s!"{path}: unknown statement kind `{k}`"
end

def param (path : String) (j : Json) : Read Param := do
  let _ ← fields path j ["type", "binds"]
  .ok { type := ← ty s!"{path}.type" (← field path j "type"),
        binds := ← bool s!"{path}.binds" (← field path j "binds") }

/-- The version of core IR this reader reads. -/
def version : Int := 1

/-- A program of Oska Core, read from the text of its core IR. -/
def program (text : String) : Read Program := do
  let j ← match Json.parse text with
    | .ok j => pure j
    | .error e => .error s!"not JSON: {e}"
  let _ ← fields "program" j ["core_ir_version", "functions"]
  let v ← int "core_ir_version" (← field "program" j "core_ir_version")
  if v ≠ version then
    throw s!"core_ir_version {v}: this Lean reference reads core IR version {version} only"
  let fs ← arr "functions" (← field "program" j "functions")
  -- The names and arities first, so a call may reach a later function.
  let heads ← list "functions" fs fun path f => do
    let _ ← fields path f ["name", "params", "ret", "body"]
    let name ← str s!"{path}.name" (← field path f "name")
    let params ← list s!"{path}.params" (← arr s!"{path}.params" (← field path f "params")) param
    pure (name, params)
  let names := heads.map (·.1)
  if let some dup := names.find? (fun n => (names.filter (· == n)).length > 1) then
    throw s!"functions: two functions are named `{dup}`"
  let arities : Arities := heads.map fun (n, ps) => (n, ps.length)
  (fs.zip heads).zip (List.range fs.length) |>.mapM fun ((f, (name, params)), i) => do
    let path := s!"functions[{i}]"
    pure { name, params,
           ret := ← ty s!"{path}.ret" (← field path f "ret"),
           body := ← expr arities s!"{path}.body" (← field path f "body") }

/-- The expression a run of program `P` starts from: a call of its `main`,
which takes no parameters and returns `()`, as an Oska `main` in Oska
Core does. -/
def entry (P : Program) : Read Expr :=
  match P.find? "main" with
  | none => .error "the program has no function `main`"
  | some f =>
    if !f.params.isEmpty then .error "`main` takes parameters; in Oska Core it takes none"
    else if f.ret ≠ .unit then .error "`main` does not return `()`"
    else .ok (.call "main" [])

end OskaFormal.Core.CoreIR
