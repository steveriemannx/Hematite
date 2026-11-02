# Hematite for VS Code

Syntax highlighting for `.hm` files: Rust plus Hematite's two signature-only additions.

```rust
fn add(a: i32, b: i32) -> ret: i32 = {
    a + b
}
```

## What is highlighted

| Token | Example | TextMate scope |
|---|---|---|
| Named-return arrow | `->` | `keyword.operator.arrow.hematite` |
| Return name (any identifier) | `ret` / `c` / `out` | `variable.parameter.return-name.hematite` |
| Colon after the return name | `:` | `punctuation.separator.colon.return-name.hematite` |
| Optional `=` before the body | `=` | `keyword.operator.assignment.fn-body.hematite` |
| Everything else | `fn`, params, types, `return`, macros, strings… | Rust grammar (`source.rust` rules) |

The same scopes apply inside closure headers, e.g. `|x: i32| -> ret: i32 = { x + 1 }`.
The scopes reuse standard prefixes (`variable.parameter`, `keyword.operator.*`), so any
color theme colors them correctly without extra work.

## Install

### Option 1: symlink (for development)

```bash
ln -s "$(pwd)" ~/.vscode/extensions/hematite-0.1.0
# then restart VS Code (or run "Developer: Reload Window")
```

Open any `.hm` file; the language mode in the status bar should read **Hematite**.

### Option 2: package a .vsix

```bash
npm install
npm run gen                       # generate the grammar first (it is git-ignored)
npx @vscode/vsce package          # produces hematite-0.1.0.vsix
code --install-extension hematite-0.1.0.vsix
```

## Development

```bash
npm install
npm run gen     # generate syntaxes/hematite.tmLanguage.json from VS Code's Rust grammar
npm test        # tokenize with vscode-textmate + vscode-oniguruma and check scopes
```

`syntaxes/hematite.tmLanguage.json` is generated and **git-ignored**, as are `*.vsix` and
`node_modules/`. Run `npm run gen` after cloning or whenever VS Code's Rust grammar changes.

`npm run gen` reads `rust.tmLanguage.json` from the VS Code installation and applies two patches:

1. insert `#hematite-signature` at the front of the `function definition` rule;
2. add rules for `-> name:` and the optional `=` before the body.

If VS Code is installed somewhere else, pass the path explicitly:

```bash
node scripts/gen-grammar.cjs /path/to/rust.tmLanguage.json
```

## Notes

- A `.hm` file may contain plain Rust; it is highlighted exactly like `.rs`.
- `fn` tokens inside `macro_rules!` templates are not affected (the named-return rule
  requires an `-> ident:` shape).
- The grammar is derived from VS Code's built-in Rust grammar, which was converted from
  [dustypomerleau/rust-syntax](https://github.com/dustypomerleau/rust-syntax) (MIT).
  The Hematite additions are released under MIT as well.
