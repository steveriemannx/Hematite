# Hematite Syntax Goals v0.4 (review draft)

> Status: draft; the M1 lowering MVP is implemented against this version (see `README.md`)
> Scope: a syntax layer for Rust, borrowing the Cpp2 (cppfront) function notation
> Files: `.hm` → lowered to `.rs` → compiled by rustc / cargo
>
> **v0.4 changes (per the latest feedback)**: the function body **never contains `ret`**.
> The v0.3 "assignment is return" sugar is removed: `ret = e;` is not part of the language;
> the body is ordinary Rust (a tail expression for the value; `return` only for early
> exits; nothing for unit functions).
> The named return `-> name: T` and the optional `=` are **signature-only declarations**
> and are erased during lowering.

---

## 0. One-liner

The two additions exist **only in the signature**; the function body is 100% normal Rust:

```rust
// 1) named return + optional `=`; the body is a tail expression
fn add(a: i32, b: i32) -> ret: i32 = { a + b }

// 2) `=` may be omitted; early exits still use ordinary `return`
fn classify(n: i32) -> label: &'static str {
    if n < 0 { return "negative"; }
    if n == 0 { return "zero"; }
    "positive"
}

// 3) unit function: nothing to return
fn log(msg: &str) -> out: () { println!("{msg}"); }

// 4) plain Rust, valid as-is
fn add_plain(a: i32, b: i32) -> i32 { a + b }
```

Lowering only "erases"; **not a single byte of the body is touched**:

```rust
fn add(a: i32, b: i32) -> ret: i32 = { a + b }
// lowered ↓
fn add(a: i32, b: i32) -> i32 { a + b }
```

---

## 1. Design principles

| # | Principle | Meaning / acceptance |
|---|---|---|
| P1 | **Rust superset** | Any valid Rust is valid Hematite and its output is byte-identical. `.rs` files can be passed to `hm` directly. |
| P2 | **Pure lowering** | The sugar is signature-only: no variables, no runtime, no body changes. The output is valid, readable Rust. |
| P3 | **No new keywords** | `ret` is not a keyword; it is just the default name, and users may still use it as an ordinary variable name. |
| P4 | **Locally decidable** | The new syntax only appears in the `fn` header; after `->` a two-token lookahead suffices, no backtracking. |
| P5 | **Position preserving** | Lowering only deletes characters on the same line and never adds or removes lines; diagnostics compensate columns, so rustc coordinates land exactly on `.hm`. |
| P6 | **No magic in the body** | No implicit declarations, assignments or tail values; what you see is what rustc sees. |

**Success criteria for v0**:

1. An example `.hm` and the equivalent hand-written `.rs` compile and run identically.
2. `hm expand` output has the same line count as the input, and `hm build` / `hm run`
   diagnostics point back to `.hm`.
3. When no Hematite-specific syntax is used, output and input are byte-identical.

---

## 2. Additions

| # | Notation | Effect |
|---|---|---|
| **G1** | Named return `-> name: Type` | Names the return value in the signature (self-documenting; a placeholder for future named-return/contract/multi-value features) |
| **G2** | Optional function-body `=` | May be written or omitted; its only meaning is enabling the default name `ret` when the name is omitted (still just a declaration) |
| **G3** | No slot name in the body | The name is not a variable and is not bound inside the body; use a tail expression (or `return` for early exits), or nothing for unit functions |

Nothing else is added.

---

## 3. Grammar (EBNF; Hematite differences highlighted)

```
item_fn   := attrs? vis? "fn" ident generics? "(" params? ")"
             ( "->" ret )? where_clause? fn_tail
fn_tail   := block                    // a normal Rust function body
           | "=" block                // G2: optional style marker, equivalent to the above
ret       := ret_slot | type          // plain Rust only has `type`
ret_slot  := ident ":" type           // G1: named return (declaration only)
```

Static rules:

- **R1** `=` must be followed by a block `{ ... }` (otherwise E0001).
- **R2** The body does not bind the slot name; an identifier of the same name in the body is
  just an ordinary identifier (usually an undeclared variable: rustc E0425, with an extra
  `hm` W0001 hint).
- **R3** Lowering does exactly two things: delete `name:` and delete the optional `=`;
  the body is preserved byte for byte.
- **R4** Any valid return type may be named (`impl Trait`, `dyn Trait`, `!`, tuples, ...) —
  the name does not participate in the type.

**Parsing strategy**: after `->`, look at two tokens — `ident ":"` (not `::`) ⇒ named return;
`ident "::"` or anything else ⇒ a normal type path. The body `=` follows the `where` clause;
track `<...>` / `(...)` / `[...]` depth so that `=` inside
`where I: Iterator<Item = i32>` or a generic default `fn f<T = u8>()` is not mistaken for
the function-body `=`.

---

## 4. Legal / illegal forms

| # | Form | Verdict | Lowering / note |
|---|---|---|---|
| 1 | `fn add(a: i32, b: i32) -> c: i32 = { a + b }` | ✅ | `fn add(a: i32, b: i32) -> i32 { a + b }` |
| 2 | `fn add(a: i32, b: i32) -> c: i32 { a + b }` | ✅ | same, `=` is optional |
| 3 | `fn add(a: i32, b: i32) -> i32 = { ret = a + b; }` | ❌ rustc E0425 | there is no `ret` variable in the body; use a tail expression (`a + b`) or an ordinary `return a + b;` |
| 4 | `fn f() -> ret: i32 = { return 1 }` | ✅ | explicitly uses the default name |
| 5 | `fn f() -> i32 = { 1 }` | ✅ | omitted name ⇒ default `ret` (declaration level) |
| 6 | `fn f() = { }` | ✅ | no return type + `=` ⇒ equivalent to `fn f() { }` |
| 7 | `fn f() -> i32 { 1 }` | ✅ | plain Rust, byte-identical |
| 8 | `fn log(m: &str) -> out: () { println!("{m}"); }` | ✅ | unit: nothing to return |
| 9 | `fn f() -> c: impl Iterator<Item = i32> { std::iter::empty() }` | ✅ | the name is only a declaration; the type is preserved |
| 10 | `trait T { fn f(&self) -> c: i32; }` | ✅ | declaration-only positions are allowed (the name is documentation) |
| 11 | `type F = fn(i32) -> i32;` | ✅ | a function pointer type is not a function definition; untouched |
| 12 | `fn f() -> c: i32 = 1` | ❌ E0001 | `=` must be followed by a block |
| 13 | closure `\|x\| -> c: T { … }` | ❌ syntax error | closures have no named return; not supported in v0 |
| 14 | `fn` templates inside `macro_rules!` | ✅ left as-is | the sugar does not reach into macros |
| 15 | `fn f() -> ret: i32 = { ret = 1; }` | ⚠️ W0001 | the body mentions the slot name and assigns to it ⇒ `hm` warns that there is no such variable; rustc then reports E0425 |
| 16 | `fn find(xs: &[i32]) -> pos: Option<usize> { if xs.is_empty() { return None; } Some(1) }` | ✅ | `return` is ordinary Rust and is used for early exits; the final value is a tail expression |

---

## 5. Lowering and semantics

Only two rules, both pure erasure:

```rust
// erase the name in the signature
fn f(a: i32) -> ret: i32 { … }   ⇒   fn f(a: i32) -> i32 { … }

// erase the optional `=` (plus the whitespace to its left, never crossing a line)
fn f(a: i32) -> i32 = { … }      ⇒   fn f(a: i32) -> i32 { … }
```

Example:

```rust
// .hm (line numbers unchanged; only same-line deletions)
fn find(xs: &[i32], k: i32) -> pos: Option<usize> = {
    if xs.len() > 100 { return None; }
    xs.iter().position(|v| *v == k)
}

// .rs
fn find(xs: &[i32], k: i32) -> Option<usize> {
    if xs.len() > 100 { return None; }
    xs.iter().position(|v| *v == k)
}
```

Semantics notes:

1. **Zero semantic difference**: the name and `=` produce no code; the body is the Rust you wrote.
2. **rustc is the authority**: types, `?`, early returns, borrows and drop order are unchanged;
   `hm` performs no data-flow analysis.
3. **What the name is for (v0)**: self-documenting signatures; a placeholder for future
   named returns / contracts / multi-value returns.
4. **Position preserving**: deletions never cross lines; columns are compensated, so
   `x.rs:3:59` maps back to `x.hm:3:73`.

---

## 6. Scope

| Position | Supported |
|---|---|
| Any `fn` definition: free functions / `pub` / `const` / `async` / `unsafe` / `extern "C"` / impl methods / default trait methods | ✅ |
| Generics / `where` clauses / lifetimes / const generics / generic defaults | ✅ |
| Trait method declarations (no body), declarations inside `extern` blocks | ✅ the name is documentation only |
| Any return type (`impl Trait` / `dyn` / `!` / tuples, ...) | ✅ the name does not affect the type |
| Closures | ❌ no named-return syntax |
| Inside `macro_rules!` / `macro` definitions | ❌ left as-is; the sugar does not reach into macros |
| `fn` generated by `quote!` and other proc macros | ❌ cannot be recognized (documented limitation) |
| `.rs` and `.hm` mixed in one crate, `.hm` files containing pure Rust, pure-Rust crates built with `hm` | ✅ superset; `.rs` copied byte for byte, plain-Rust input equals output |

---

## 7. Formatting (`hm fmt`, draft, not implemented in v0)

```rust
fn add(a: i32, b: i32) -> ret: i32 { a + b }
fn add(a: i32, b: i32) -> i32 = { a + b }
```

- One space before `->`; no space between `name` and `:`, one space after `:`; when `=` is
  written it has one space on each side.
- No line re-flowing; only whitespace normalization.

---

## 8. Diagnostics

| Code | Source | Meaning |
|---|---|---|
| E0001 | hm | `=` must be followed by a block |
| E0425, etc. | rustc | the body uses an undeclared `ret` (the name is not bound in the body) |
| W0001 | hm | the body mentions the slot name and assigns to it (e.g. `ret = 1;`) ⇒ hint that there is no such variable; use a tail expression, or `return` for an early exit |
| position mapping | hm | rustc's `file:line:col` maps back to `.hm` (lines unchanged, columns compensated) |

Example:

```
hm: warning: examples/x.hm:5:12: named return `ret` is only a signature declaration; ...
error[E0425]: cannot find value `ret` in this scope
 --> examples/x.hm:5:5
```

---

## 9. Non-goals (explicitly out of scope for v0)

- A readable/writable return variable inside the body (the v0.2 design, removed)
- "Assignment is return" sugar (the v0.3 design, removed in this version: `ret` must not appear in the body)
- Multi-value returns `-> (a: T, b: U)`
- Cpp2 parameter kinds: `out` / `inout` / `move` / `forward`
- Expression bodies `= expr` (`= { … }` covers the need)
- Sugar for closures / function types
- Any change to expression grammar, operators or existing token meanings
- A custom type checker / macro system / runtime library

---

## 10. Open questions

| # | Question | Recommendation |
|---|---|---|
| Q1 | Confirm that the name and `=` **do not change semantics** and that `ret` never appears in the body | Confirm |
| Q2 | Should the name gain semantics in v0.5 (named-return calls / contracts / multi-value returns)? | Keep the syntax slot for now; decide later |
| Q3 | Support `-> _: T` (discard the name)? | Support it; equivalent to anonymous |
| Q4 | Keep `=`? It is now a purely visual marker plus the default-name entry point | Keep it (the cpp2 flavour you originally asked for) |
| Q5 | File extension `.hm`, command `hm` (`expand`/`build`/`run`) | Implemented in the MVP as specified |

---

## 11. Milestones

| Stage | Content | Status |
|---|---|---|
| M0 | Spec approval | waiting for your confirmation |
| M1 | Position-preserving lowering MVP (token level, no type checking) | ✅ implemented: `hm expand` / `hm build` / `hm run`, diagnostic mapping and matrix tests |
| M2 | Cargo integration (multi-file crates) | ✅ implemented: mirrors the crate into `target/hematite/<name>/`, `.hm` → `.rs`, runs `cargo build/run`; diagnostics map back to `.hm` |
| M3 | Complete E0xxx/W0001 + `hm explain` + `hm fmt` | todo |
| M4 | Works with rust-analyzer (LSP pointing at `target/hematite/` generated code) | todo |
| M5 | The name gains semantics (named returns / contracts / multi-value returns) | to be reviewed separately |

---

## 12. Changelog

### v0.4 (this version)

- **No `ret` in the body**: the v0.3 "assignment is return" sugar is removed; `ret = e;`
  is no longer part of the language.
- Named return and `=` converge into **signature-only declarations**; lowering is erasure
  (the body is byte-identical).
- Diagnostics simplified to E0001 + W0001; the matrix was rewritten with 15 rows.
- M1 is implemented: token-level erasure + position mapping + `hm expand/build/run`.

### v0.3

- Zero variables in the body; `ret = e;` was treated as sugar for `return e;`. Removed in this version.

### v0.2

- `=` became optional. Adopted the "slot variable + implicit tail value" design, later removed.

### v0.1

- First draft: `-> name: Type = block`.

---

## Appendix: cpp2 ↔ Hematite

| Concept | Cpp2 | Hematite v0.4 |
|---|---|---|
| Function header | `add: (a: i32, b: i32) -> i32 =` | `fn add(a: i32, b: i32) -> i32` (`=` optional) |
| Named return | `-> (ret: i32) =` (return parameter list) | `-> ret: i32` (single return, no parens, declaration only) |
| Return variable | writable variable in the body, returned at the end | **no variable**; use a tail expression (or `return` for early exits) in the body |
| Default return name | none | `ret` (only available when `=` is written; declaration level) |
| Function body | `= { … }` or `= expr;` | only `{ … }` (`=` optional) |
| Multi-value returns | `-> (a: T, b: U)` | non-goal (v0) |
| Compiler | cppfront lowers to Cpp1 | `hm` lowers to Rust, then rustc takes over |
