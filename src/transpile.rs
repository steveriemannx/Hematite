//! Erase Hematite syntax down to plain Rust.
//!
//! v0.4 has exactly two additions, both in the `fn` header, and both are pure
//! deletions:
//!   1. drop the `name:` part of `-> name: Type`;
//!   2. drop the optional `=` (plus the spaces/tabs to its left on the same line)
//!      that precedes the function body.
//!
//! Deletions never span a line break, so the output has exactly as many lines as
//! the input. Columns are compensated through `Expansion::map_line_col`, which lets
//! rustc diagnostics map back to the original `.hm` file precisely.

use crate::lexer::{tokenize, Kind, Token};

#[derive(Debug)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl Error {
    pub fn line_col(&self, src: &str) -> (usize, usize) {
        line_col(src, &line_starts(src), self.offset)
    }
}

#[derive(Debug)]
pub struct Warning {
    pub offset: usize,
    pub message: String,
}

#[derive(Debug)]
pub struct Expansion {
    /// Lowered Rust source (same line count as the input).
    pub output: String,
    /// Warnings such as using the return name inside the body (offsets refer to
    /// the original `.hm` source).
    pub warnings: Vec<Warning>,
    /// Deleted byte ranges in the original source (sorted and merged).
    deletions: Vec<(usize, usize)>,
    orig: String,
    orig_line_starts: Vec<usize>,
    out_line_starts: Vec<usize>,
}

impl Expansion {
    pub fn orig_line_col(&self, offset: usize) -> (usize, usize) {
        line_col(&self.orig, &self.orig_line_starts, offset)
    }

    /// Map a 1-based `(line, col)` in the generated file back to the original
    /// `.hm` `(line, col)`.
    pub fn map_line_col(&self, line: usize, col: usize) -> (usize, usize) {
        let out_off = offset_for_line_col(&self.output, &self.out_line_starts, line, col);
        let orig_off = self.map_offset(out_off);
        self.orig_line_col(orig_off)
    }

    fn map_offset(&self, out_off: usize) -> usize {
        let mut deleted_before = 0usize;
        for &(ds, de) in &self.deletions {
            let out_start = ds - deleted_before;
            if out_off >= out_start {
                deleted_before += de - ds;
            } else {
                break;
            }
        }
        out_off + deleted_before
    }
}

pub fn expand(src: &str) -> Result<Expansion, Error> {
    let toks = tokenize(src);
    let mut deletions: Vec<(usize, usize)> = Vec::new();
    let mut warnings: Vec<Warning> = Vec::new();

    let mut i = 0;
    while i < toks.len() {
        let t = toks[i];
        if t.kind == Kind::Ident {
            let text = t.text(src);
            if text == "macro_rules" || text == "macro" {
                if let Some(j) = skip_macro(&toks, i) {
                    i = j;
                    continue;
                }
            }
            if text == "fn" && toks.get(i + 1).map(|t| t.kind) == Some(Kind::Ident) {
                i = parse_fn(src, &toks, i, &mut deletions, &mut warnings)?;
                continue;
            }
        }
        // A closure return annotation starts right after the closing `|`.
        if t.kind == Kind::FatArrow
            && i > 0
            && matches!(toks[i - 1].kind, Kind::Pipe | Kind::PipePipe)
        {
            i = parse_closure_return(src, &toks, i, &mut deletions)?;
            continue;
        }
        i += 1;
    }

    finish(src, deletions, warnings)
}

fn parse_fn(
    src: &str,
    toks: &[Token],
    fn_idx: usize,
    deletions: &mut Vec<(usize, usize)>,
    warnings: &mut Vec<Warning>,
) -> Result<usize, Error> {
    let mut i = fn_idx + 2; // skip `fn` and the function name

    // Generic parameters (possibly with defaults: `fn f<T = u8>()`).
    if kind_at(toks, i) == Some(Kind::Open('<')) {
        i = skip_angle(toks, i);
    }
    // Parameter list.
    if kind_at(toks, i) != Some(Kind::Open('(')) {
        return Ok(i);
    }
    i = skip_group(toks, i);

    // Return type (possibly with a named return slot).
    let mut slot: Option<(usize, usize, usize)> = None; // (name token, del start, del end)
    if kind_at(toks, i) == Some(Kind::FatArrow) {
        i += 1;
        if let (Some(name), Some(colon)) = (toks.get(i), toks.get(i + 1)) {
            if name.kind == Kind::Ident && colon.kind == Kind::Colon {
                let start = extend_left_ws(src, name.start);
                slot = Some((i, start, colon.end));
                i += 2; // skip `name :`; the real type follows
            }
        }
        i = scan_type(src, toks, i);
    }

    // `where` clause.
    if let Some(t) = toks.get(i) {
        if t.kind == Kind::Ident && t.text(src) == "where" {
            i = scan_where(toks, i + 1);
        }
    }

    // Optional function-body `=`.
    let mut eq: Option<(usize, usize)> = None;
    if kind_at(toks, i) == Some(Kind::Eq) {
        let t = toks[i];
        eq = Some((extend_left_ws(src, t.start), t.end));
        i += 1;
    }

    let body = if kind_at(toks, i) == Some(Kind::Open('{')) {
        Some(i)
    } else {
        None
    };

    if body.is_none() && eq.is_some() {
        return Err(Error {
            offset: eq.unwrap().1 - 1,
            message: "`=` must be followed by the function body `{ ... }`".into(),
        });
    }

    if let Some((name_idx, start, end)) = slot {
        push_deletion(src, deletions, start, end)?;
        if let Some(body_idx) = body {
            let body_end = skip_group(toks, body_idx);
            let name = toks[name_idx].text(src);
            let mut j = body_idx + 1;
            while j + 1 < body_end {
                if toks[j].kind == Kind::Ident
                    && toks[j].text(src) == name
                    && toks[j + 1].kind == Kind::Eq
                {
                    warnings.push(Warning {
                        offset: toks[j].start,
                        message: format!(
                            "named return `{name}` is only a signature declaration; it is not a \
                             variable inside the function body - use a normal `return` (or a tail \
                             expression) to return a value"
                        ),
                    });
                }
                j += 1;
            }
        }
    }

    if let Some((start, end)) = eq {
        push_deletion(src, deletions, start, end)?;
    }

    Ok(i)
}

/// Handle a closure return annotation: `|args| -> [name:] Type [=] { body }`.
///
/// Closures with an explicit return type always have a block body in Rust, so this
/// only has to delete `name:` and (when present) the Hematite `=` before `{`.
fn parse_closure_return(
    src: &str,
    toks: &[Token],
    arrow_idx: usize,
    deletions: &mut Vec<(usize, usize)>,
) -> Result<usize, Error> {
    let mut i = arrow_idx + 1;

    // Optional named return slot.
    if let (Some(name), Some(colon)) = (toks.get(i), toks.get(i + 1)) {
        if name.kind == Kind::Ident && colon.kind == Kind::Colon {
            let start = extend_left_ws(src, name.start);
            push_deletion(src, deletions, start, colon.end)?;
            i += 2;
        }
    }

    // Walk the return type until the body block (or the optional `=`).
    let mut depth = 0i32;
    while i < toks.len() {
        match toks[i].kind {
            Kind::Open(c) => {
                if depth == 0 && c == '{' {
                    return Ok(i);
                }
                depth += 1;
            }
            Kind::Close(_) if depth > 0 => depth -= 1,
            Kind::Close(_) => return Ok(i),
            Kind::Eq if depth == 0 => {
                // Only the `=` directly in front of the body block is Hematite syntax.
                if kind_at(toks, i + 1) == Some(Kind::Open('{')) {
                    let t = toks[i];
                    push_deletion(src, deletions, extend_left_ws(src, t.start), t.end)?;
                    return Ok(i + 1);
                }
                return Ok(i);
            }
            Kind::Semi if depth == 0 => return Ok(i),
            _ => {}
        }
        i += 1;
    }
    Ok(i)
}

fn scan_type(src: &str, toks: &[Token], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < toks.len() {
        match toks[i].kind {
            Kind::Open(c) => {
                if depth == 0 && c == '{' {
                    return i;
                }
                depth += 1;
            }
            Kind::Close(_) if depth > 0 => depth -= 1,
            Kind::Eq if depth == 0 => return i,
            Kind::Semi if depth == 0 => return i,
            Kind::Ident if depth == 0 && toks[i].text(src) == "where" => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

fn scan_where(toks: &[Token], mut i: usize) -> usize {
    let mut depth = 0i32;
    while i < toks.len() {
        match toks[i].kind {
            Kind::Open(c) => {
                if depth == 0 && c == '{' {
                    return i;
                }
                depth += 1;
            }
            Kind::Close(_) if depth > 0 => depth -= 1,
            Kind::Eq if depth == 0 => return i,
            Kind::Semi if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

/// Skip a `(...)`, `[...]` or `{...}` group: only these three brackets count.
/// In ordinary Rust code `<` and `>` can also be comparison operators, so they must
/// never participate in this matching.
fn skip_group(toks: &[Token], start: usize) -> usize {
    let mut depth = 0i32;
    let mut j = start;
    while j < toks.len() {
        match toks[j].kind {
            Kind::Open('(') | Kind::Open('[') | Kind::Open('{') => depth += 1,
            Kind::Close(')') | Kind::Close(']') | Kind::Close('}') => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    j
}

/// Skip a `<...>` generic parameter group.
fn skip_angle(toks: &[Token], start: usize) -> usize {
    let mut depth = 0i32;
    let mut j = start;
    while j < toks.len() {
        match toks[j].kind {
            Kind::Open('<') => depth += 1,
            Kind::Close('>') => {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            }
            _ => {}
        }
        j += 1;
    }
    j
}

/// Skip the token tree of `macro_rules! name { ... }` / `macro name { ... }`.
/// A `fn` inside a macro template must not be rewritten as a real function.
fn skip_macro(toks: &[Token], i: usize) -> Option<usize> {
    let mut j = i + 1;
    if toks.get(j).map(|t| t.kind) == Some(Kind::Ident) {
        j += 1;
    }
    let mut guard = 0;
    while j < toks.len() && guard < 8 {
        match toks[j].kind {
            Kind::Open('{') | Kind::Open('(') | Kind::Open('[') => {
                return Some(skip_group(toks, j));
            }
            Kind::Semi => return None,
            _ => {}
        }
        j += 1;
        guard += 1;
    }
    None
}

fn kind_at(toks: &[Token], i: usize) -> Option<Kind> {
    toks.get(i).map(|t| t.kind)
}

fn extend_left_ws(src: &str, mut start: usize) -> usize {
    let b = src.as_bytes();
    while start > 0 && (b[start - 1] == b' ' || b[start - 1] == b'\t') {
        start -= 1;
    }
    start
}

fn push_deletion(
    src: &str,
    deletions: &mut Vec<(usize, usize)>,
    start: usize,
    end: usize,
) -> Result<(), Error> {
    if start >= end {
        return Ok(());
    }
    if src[start..end].contains('\n') {
        return Err(Error {
            offset: start,
            message: "internal error: deletion range crosses a line break".into(),
        });
    }
    deletions.push((start, end));
    Ok(())
}

fn finish(
    src: &str,
    mut deletions: Vec<(usize, usize)>,
    warnings: Vec<Warning>,
) -> Result<Expansion, Error> {
    deletions.sort_by_key(|d| d.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in deletions {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }

    let mut output = String::with_capacity(src.len());
    let mut pos = 0usize;
    for &(s, e) in &merged {
        output.push_str(&src[pos..s]);
        pos = e;
    }
    output.push_str(&src[pos..]);

    let out_line_starts = line_starts(&output);
    Ok(Expansion {
        output,
        warnings,
        deletions: merged,
        orig: src.to_string(),
        orig_line_starts: line_starts(src),
        out_line_starts,
    })
}

pub fn line_starts(s: &str) -> Vec<usize> {
    let mut v = vec![0usize];
    for (i, b) in s.bytes().enumerate() {
        if b == b'\n' {
            v.push(i + 1);
        }
    }
    v
}

fn line_col(s: &str, starts: &[usize], offset: usize) -> (usize, usize) {
    let offset = offset.min(s.len());
    let idx = starts.partition_point(|&st| st <= offset).saturating_sub(1);
    let start = starts[idx];
    let col = s[start..offset].chars().count() + 1;
    (idx + 1, col)
}

fn offset_for_line_col(s: &str, starts: &[usize], line: usize, col: usize) -> usize {
    if starts.is_empty() {
        return 0;
    }
    let idx = line.saturating_sub(1).min(starts.len() - 1);
    let start = starts[idx];
    let end = starts.get(idx + 1).copied().unwrap_or(s.len());
    let text = &s[start..end];
    let byte = text
        .char_indices()
        .nth(col.saturating_sub(1))
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    start + byte
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x(src: &str) -> String {
        expand(src).expect("expand failed").output
    }

    #[test]
    fn pure_rust_is_byte_identical() {
        let src = "fn add(a: i32, b: i32) -> i32 { a + b }\n";
        assert_eq!(x(src), src);
    }

    #[test]
    fn user_example_with_equals() {
        let src = "fn add(a: i32, b: i32) -> c: i32 = { return a + b }";
        assert_eq!(x(src), "fn add(a: i32, b: i32) -> i32 { return a + b }");
    }

    #[test]
    fn user_example_without_equals() {
        let src = "fn add(a: i32, b: i32) -> c: i32 { return a + b }";
        assert_eq!(x(src), "fn add(a: i32, b: i32) -> i32 { return a + b }");
    }

    #[test]
    fn default_name_with_equals_only() {
        assert_eq!(x("fn f() -> i32 = { 1 }"), "fn f() -> i32 { 1 }");
    }

    #[test]
    fn no_return_type_with_equals() {
        assert_eq!(x("fn f() = { }"), "fn f() { }");
    }

    #[test]
    fn where_clause_item_binding_untouched() {
        let src = "fn f<I>() -> i32 where I: Iterator<Item = i32> { 1 }";
        assert_eq!(x(src), src);
    }

    #[test]
    fn generic_defaults_untouched() {
        let src = "fn f<T = u8>() -> T { t }";
        assert_eq!(x(src), src);
    }

    #[test]
    fn impl_trait_return_with_slot() {
        let src = "fn f() -> c: impl Iterator<Item = i32> { std::iter::empty() }";
        assert_eq!(x(src), "fn f() -> impl Iterator<Item = i32> { std::iter::empty() }");
    }

    #[test]
    fn fn_pointer_type_untouched() {
        let src = "type F = fn(i32) -> i32;";
        assert_eq!(x(src), src);
    }

    #[test]
    fn strings_untouched() {
        let src = r#"fn f() { let s = "fn g() -> c: i32 = {}"; }"#;
        assert_eq!(x(src), src);
    }

    #[test]
    fn comments_untouched() {
        let src = "// fn g() -> c: i32 = {}\nfn f() { }";
        assert_eq!(x(src), src);
    }

    #[test]
    fn macro_rules_untouched() {
        let src = "macro_rules! m {\n    () => { fn g() -> c: i32 = { 1 } };\n}\nfn f() { }\n";
        assert_eq!(x(src), src);
    }

    #[test]
    fn nested_fn_transformed() {
        let src = "fn outer() { fn inner() -> ret: i32 = { 1 } inner(); }";
        assert_eq!(x(src), "fn outer() { fn inner() -> i32 { 1 } inner(); }");
    }

    #[test]
    fn impl_method_transformed() {
        let src = "impl S { pub fn get(&self) -> c: i32 = { return self.x; } }";
        assert_eq!(x(src), "impl S { pub fn get(&self) -> i32 { return self.x; } }");
    }

    #[test]
    fn trait_decl_transformed() {
        let src = "trait T { fn f(&self) -> c: i32; }";
        assert_eq!(x(src), "trait T { fn f(&self) -> i32; }");
    }

    #[test]
    fn line_count_preserved_for_multiline_signature() {
        let src = "fn f(\n    a: i32,\n) -> c: i32\n{\n    return a;\n}\n";
        let out = x(src);
        assert_eq!(src.lines().count(), out.lines().count());
        assert!(out.contains(") -> i32\n"), "unexpected output: {out}");
    }

    #[test]
    fn warns_when_body_assigns_slot_name() {
        let exp = expand("fn f() -> ret: i32 = { ret = 1; }").unwrap();
        assert_eq!(exp.output, "fn f() -> i32 { ret = 1; }");
        assert_eq!(exp.warnings.len(), 1);
        let (line, _col) = exp.orig_line_col(exp.warnings[0].offset);
        assert_eq!(line, 1);
    }

    #[test]
    fn closure_named_return_with_equals() {
        let src = "let f = |x: i32| -> ret: i32 = { x + 1 };";
        assert_eq!(x(src), "let f = |x: i32| -> i32 { x + 1 };");
    }

    #[test]
    fn closure_named_return_without_equals() {
        let src = "let f = |x: i32| -> c: i32 { x + 1 };";
        assert_eq!(x(src), "let f = |x: i32| -> i32 { x + 1 };");
    }

    #[test]
    fn empty_closure_named_return() {
        let src = "let f = || -> ret: i32 = { 1 };";
        assert_eq!(x(src), "let f = || -> i32 { 1 };");
    }

    #[test]
    fn plain_closure_with_return_type_untouched() {
        let src = "let f = |x: i32| -> i32 { x + 1 };";
        assert_eq!(x(src), src);
    }

    #[test]
    fn function_pointer_type_in_let_untouched() {
        let src = "let f: fn(i32) -> i32 = square;";
        assert_eq!(x(src), src);
    }

    #[test]
    fn closure_with_fn_pointer_return_type() {
        let src = "let f = |x: i32| -> fn(i32) -> i32 = { g };";
        assert_eq!(x(src), "let f = |x: i32| -> fn(i32) -> i32 { g };");
    }

    #[test]
    fn closure_inside_fn_body_transformed() {
        let src = "fn outer() { let f = |x: i32| -> r: i32 = { x }; }";
        assert_eq!(x(src), "fn outer() { let f = |x: i32| -> i32 { x }; }");
    }

    #[test]
    fn boolean_or_and_bitwise_or_untouched() {
        let src = "let x = a | b || c;";
        assert_eq!(x(src), src);
    }

    #[test]
    fn equals_after_non_block_is_an_error() {
        assert!(expand("fn f() -> c: i32 = 1").is_err());
    }

    #[test]
    fn body_positions_map_back_exactly() {
        let src = "fn add(a: i32, b: i32) -> c: i32 = { return a + b }";
        let exp = expand(src).unwrap();
        // Position of the body's "return" in the output.
        let out_idx = exp.output.find("return").unwrap();
        let line = 1 + exp.output[..out_idx].matches('\n').count();
        let line_start = exp.output[..out_idx].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = exp.output[line_start..out_idx].chars().count() + 1;
        let (ol, oc) = exp.map_line_col(line, col);
        let orig_idx = src.find("return").unwrap();
        assert_eq!((ol, oc), (1, src[..orig_idx].chars().count() + 1));
    }

    #[test]
    fn deletion_never_removes_newlines() {
        let src = "fn f()\n    -> c: i32\n    = {\n    1\n}\n";
        let out = x(src);
        assert_eq!(src.lines().count(), out.lines().count());
    }
}
