import OskaFormal.Core.CoreIR

/-!
# The Lean reference

`oska_core [--fuel N] [FILE]` reads a program of Oska Core as core IR
(from FILE, or standard input without one), runs its `main` with the
executable semantics of `OskaFormal.Core.Eval`, and prints how the run
ended, in the form the differential test (`crates/oska-run/tests/
lean_reference.rs`) compares with the interpreter:

    result ()          main returned this value
    exit 0             and the program exits with 0

    panic P0001        a panic, with the interpreter's code
    exit 101           and the program exits with 101

It exits with 0 once it has printed that. It prints nothing on standard
output, says what is wrong on standard error and exits with 2 when the
input is not core IR version 1 (or the arguments are wrong), with 3 when
the run gets stuck, and with 4 when it runs out of fuel, N steps (by
default 100000000).
-/

open OskaFormal.Core

/-- A value as the reference prints it (`run` returns only values). -/
def render : Expr → String
  | .unit => "()"
  | .int n => toString n
  | .bool b => toString b
  | _ => "(not a value)"

/-- The interpreter's code for a panic (`docs/runtime.md`). -/
def panicCode : PanicKind → String
  | .overflow => "P0001"
  | .divisionByZero => "P0004"

def defaultFuel : Nat := 100000000

def usage : String := "usage: oska_core [--fuel N] [FILE]"

/-- The fuel and the file the arguments name. -/
def parseArgs : List String → Nat → Option String → Except String (Nat × Option String)
  | [], fuel, file => .ok (fuel, file)
  | "--fuel" :: n :: rest, _, file =>
    match n.toNat? with
    | some fuel => parseArgs rest fuel file
    | none => .error s!"--fuel {n}: expected a number of steps"
  | arg :: rest, fuel, none =>
    if arg.startsWith "-" then .error s!"unknown option {arg}" else parseArgs rest fuel (some arg)
  | arg :: _, _, some _ => .error s!"unexpected argument {arg}"

def main (args : List String) : IO UInt32 := do
  let stderr ← IO.getStderr
  match parseArgs args defaultFuel none with
  | .error e =>
    stderr.putStrLn s!"error: {e}\n{usage}"
    return 2
  | .ok (fuel, file) =>
    let text ← match file with
      | some path => IO.FS.readFile path
      | none => do (← IO.getStdin).readToEnd
    let read : Except String (Program × Expr) := match CoreIR.program text with
      | .error e => .error s!"not core IR version {CoreIR.version}: {e}"
      | .ok P => match CoreIR.entry P with
        | .error e => .error s!"no program to run: {e}"
        | .ok e => .ok (P, e)
    match read with
    | .error e =>
      stderr.putStrLn s!"error: {e}"
      return 2
    | .ok (P, e) =>
      match run P fuel e with
      | .value v =>
        IO.println s!"result {render v}\nexit 0"
        return 0
      | .panic k =>
        IO.println s!"panic {panicCode k}\nexit 101"
        return 0
      | .stuck _ =>
        stderr.putStrLn "error: the run is stuck: an expression that is not a value takes no step"
        return 3
      | .outOfFuel =>
        stderr.putStrLn s!"error: the run did not end within {fuel} steps (--fuel)"
        return 4
