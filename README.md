# Hematite

Hematite is a **syntax layer for Rust**: a function signature may name its return value and
put an optional `=` before the body. `hm` erases both before handing the file to rustc.

```rust
fn add(a: i32, b: i32) -> ret: i32 = { a + b }
// lowered to
fn add(a: i32, b: i32) -> i32 { a + b }
```

- `-> name: Type`: named return — a signature-only declaration (self-documenting)
- `= { ... }`: optional `=` — also signature-only; when the name is omitted the default name is `ret`
- The braces contain 100% normal Rust: a tail expression for the value (early exits use `return`),
  nothing for unit functions
- There is **no** `ret` variable inside the body; lowering deletes only the two signature tokens
  and leaves the body byte for byte unchanged
- A trailing `return` is unnecessary and `clippy::needless_return` flags it, so all examples here use
  tail expressions and keep `return` for early exits only

Full spec: [docs/syntax-goals.md](docs/syntax-goals.md) (v0.4).

## Usage

```bash
cargo build --release

# single file: lower only, print the generated Rust
./target/release/hm expand examples/demo.hm

# single file: lower + rustc build (artifacts in target/hematite/)
./target/release/hm build examples/demo.hm

# single file: lower + build + run
./target/release/hm run examples/demo.hm

# multi-file project: any directory that contains a Cargo.toml
#   - the crate is mirrored into target/hematite/<name>/
#   - .hm becomes .rs, everything else is copied (target/ and .git/ are skipped)
#   - cargo build / cargo run then runs inside the mirror
./target/release/hm build examples/project
./target/release/hm run   examples/project
```

`hm` keeps line numbers: the generated `.rs` has the same number of lines as the input `.hm`,
and rustc diagnostics map back to `.hm` (lines unchanged, columns compensated for deletions).

## Using it with Rust

Hematite is a superset of Rust: any valid Rust is valid Hematite with unchanged semantics.

1. **Mix in the same crate**: `.hm` and `.rs` files can `mod` each other. `hm build` only lowers
   `.hm` files and copies `.rs` files byte for byte (`examples/project` has `main.hm` calling
   into plain-Rust `util.rs`).
2. **`.hm` files can be entirely plain Rust**: no Hematite syntax required; output equals input
   byte for byte.
3. **Pure-Rust projects work too**: `hm build` / `hm run` are just a cargo wrapper; the artifact
   is still ordinary Rust.

Limitations:

- `.hm` files must be built through `hm build` / `hm run` (cargo does not know `.hm`)
- `fn` inside `macro_rules!` / `macro` templates is left alone; the sugar does not reach into macros
- rust-analyzer does not understand `.hm` directly yet (planned in M4)

## Editor support (VS Code)

`editors/vscode/` is a syntax highlighting extension for `.hm` (named returns, the body `=`,
everything else reuses the Rust grammar).

```bash
cd editors/vscode
npm install
npm run gen               # generate syntaxes/hematite.tmLanguage.json (git-ignored)
npm test                  # tokenize with vscode-textmate and check the scopes

# dev: symlink into the VS Code extensions directory, then reload the window
ln -s "$(pwd)" ~/.vscode/extensions/hematite-0.1.0

# or package and install
npx @vscode/vsce package
code --install-extension hematite-0.1.0.vsix
```

The TextMate grammar is generated from VS Code's built-in Rust grammar by `npm run gen` and is
**not committed** (like the packaged `.vsix` and `node_modules/`). Run `npm run gen` after
cloning and before packaging.

See [editors/vscode/README.md](editors/vscode/README.md).

## Tests

```bash
cargo test
```

Coverage: byte-identical passthrough for plain Rust, both signature styles, `=` inside
`where` associated-type bindings, generic defaults, `impl Trait` returns, function pointer
types, strings/comments, `macro_rules!`, nested `fn`, trait declarations, line-count
preservation, diagnostic position mapping.

A few examples:

- `examples/demo.hm`: single file covering named returns / `=` / unit functions / pure Rust
- `examples/project/`: multi-file cargo crate (`src/main.hm` + `src/math.hm` + plain-Rust `src/util.rs`)

## Layout

```
docs/syntax-goals.md   # syntax goals (v0.4 review draft)
src/lexer.rs           # small tokenizer: skips comments/strings/char literals/lifetimes
src/transpile.rs       # lowering: delete `name:` and the optional `=`, plus position mapping
src/main.rs            # hm CLI: expand / build / run (single file + crate) + diagnostic mapping
examples/demo.hm       # single-file example
examples/project/      # multi-file cargo crate example (main.hm + math.hm + util.rs)
editors/vscode/        # VS Code syntax highlighting extension (.hm)
```
