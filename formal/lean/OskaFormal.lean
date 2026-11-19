-- The root of the Oska Core library: it imports every module, so
-- `lake build` checks every proof.
import OskaFormal.Basic
import OskaFormal.Core.Syntax
import OskaFormal.Core.Typing
import OskaFormal.Core.Semantics
import OskaFormal.Core.Soundness
import OskaFormal.Core.Eval
import OskaFormal.Core.CoreIR
import OskaFormal.Core.Examples
