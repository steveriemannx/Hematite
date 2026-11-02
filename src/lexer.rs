//! A deliberately small Rust tokenizer.
//!
//! `hm` only cares about the `fn` header, so the tokenizer just has to skip
//! comments, strings and char/lifetime literals correctly, and emit identifiers,
//! `->`, `:`, `::`, `=`, `;` and brackets. Every other token is ignored.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ident,
    FatArrow,
    Colon,
    ColonColon,
    Eq,
    Semi,
    Pipe,
    PipePipe,
    Open(char),
    Close(char),
}

#[derive(Debug, Clone, Copy)]
pub struct Token {
    pub kind: Kind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub fn text<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let b = src.as_bytes();
    let len = b.len();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < len {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if b.get(i + 1) == Some(&b'/') => {
                i += 2;
                while i < len && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => i = skip_block_comment(b, i),
            b'"' => i = skip_quoted(b, i + 1, b'"'),
            b'\'' => i = skip_char_or_lifetime(b, i),
            _ => {
                if let Some(j) = skip_prefixed_literal(b, i) {
                    i = j;
                    continue;
                }
                if is_ident_start(b, i) {
                    let (kind, j) = scan_ident(b, i);
                    toks.push(Token { kind, start: i, end: j });
                    i = j;
                    continue;
                }
                if c.is_ascii_digit() {
                    let mut j = i;
                    while j < len
                        && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.')
                    {
                        j += 1;
                    }
                    i = j;
                    continue;
                }
                match c {
                    b'-' if b.get(i + 1) == Some(&b'>') => {
                        toks.push(Token { kind: Kind::FatArrow, start: i, end: i + 2 });
                        i += 2;
                    }
                    b':' if b.get(i + 1) == Some(&b':') => {
                        toks.push(Token { kind: Kind::ColonColon, start: i, end: i + 2 });
                        i += 2;
                    }
                    b':' => {
                        toks.push(Token { kind: Kind::Colon, start: i, end: i + 1 });
                        i += 1;
                    }
                    // `==` is not the function-body assignment, and `=>` is not a
                    // closing angle bracket: swallow both as a unit.
                    b'=' if matches!(b.get(i + 1), Some(&b'=') | Some(&b'>')) => i += 2,
                    b'=' => {
                        toks.push(Token { kind: Kind::Eq, start: i, end: i + 1 });
                        i += 1;
                    }
                    b';' => {
                        toks.push(Token { kind: Kind::Semi, start: i, end: i + 1 });
                        i += 1;
                    }
                    // `|` opens/closes closure parameters; `||` is an empty one.
                    b'|' if b.get(i + 1) == Some(&b'|') => {
                        toks.push(Token { kind: Kind::PipePipe, start: i, end: i + 2 });
                        i += 2;
                    }
                    b'|' => {
                        toks.push(Token { kind: Kind::Pipe, start: i, end: i + 1 });
                        i += 1;
                    }
                    b'(' | b'[' | b'{' => {
                        toks.push(Token { kind: Kind::Open(c as char), start: i, end: i + 1 });
                        i += 1;
                    }
                    b')' | b']' | b'}' => {
                        toks.push(Token { kind: Kind::Close(c as char), start: i, end: i + 1 });
                        i += 1;
                    }
                    b'<' => {
                        toks.push(Token { kind: Kind::Open('<'), start: i, end: i + 1 });
                        i += 1;
                    }
                    b'>' => {
                        toks.push(Token { kind: Kind::Close('>'), start: i, end: i + 1 });
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
        }
    }
    toks
}

fn skip_block_comment(b: &[u8], i: usize) -> usize {
    let mut depth = 0usize;
    let mut j = i;
    while j < b.len() {
        if b[j] == b'/' && b.get(j + 1) == Some(&b'*') {
            depth += 1;
            j += 2;
        } else if b[j] == b'*' && b.get(j + 1) == Some(&b'/') {
            depth = depth.saturating_sub(1);
            j += 2;
            if depth == 0 {
                return j;
            }
        } else {
            j += 1;
        }
    }
    b.len()
}

fn skip_quoted(b: &[u8], mut j: usize, quote: u8) -> usize {
    while j < b.len() {
        if b[j] == b'\\' {
            j += 2;
            continue;
        }
        if b[j] == quote {
            return j + 1;
        }
        j += 1;
    }
    b.len()
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 {
        1
    } else if first >> 5 == 0b110 {
        2
    } else if first >> 4 == 0b1110 {
        3
    } else {
        4
    }
}

/// Handle `'a` (lifetime), `'x'`, `'\n'`, `'é'` in one place.
fn skip_char_or_lifetime(b: &[u8], i: usize) -> usize {
    let len = b.len();
    let mut j = i + 1;
    if j >= len {
        return len;
    }
    if b[j] == b'\\' {
        let mut k = j;
        while k < len {
            if b[k] == b'\\' {
                k += 2;
                continue;
            }
            if b[k] == b'\'' {
                return k + 1;
            }
            k += 1;
        }
        return len;
    }
    let ch_end = j + utf8_len(b[j]);
    if ch_end < len && b[ch_end] == b'\'' {
        return ch_end + 1;
    }
    // Lifetime: 'a / 'static / '_
    while j < len && is_ident_continue(b[j]) {
        j += 1;
    }
    j
}

/// Handle prefixed literals such as `r".."`, `r#".."#`, `b".."`, `b'x'`,
/// `br#".."#` and `c".."`.
fn skip_prefixed_literal(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'r' => raw_string(b, i),
        b'b' => match b.get(i + 1)? {
            b'"' => Some(skip_quoted(b, i + 2, b'"')),
            b'\'' => Some(skip_char_or_lifetime(b, i + 1)),
            b'r' => raw_string(b, i + 1),
            _ => None,
        },
        b'c' => match b.get(i + 1)? {
            b'"' => Some(skip_quoted(b, i + 2, b'"')),
            b'r' => raw_string(b, i + 1),
            _ => None,
        },
        _ => None,
    }
}

fn raw_string(b: &[u8], r_pos: usize) -> Option<usize> {
    let mut j = r_pos + 1;
    let mut hashes = 0usize;
    while b.get(j) == Some(&b'#') {
        hashes += 1;
        j += 1;
    }
    if b.get(j) != Some(&b'"') {
        return None;
    }
    j += 1;
    while j < b.len() {
        if b[j] == b'"' {
            let mut k = j + 1;
            let mut h = 0usize;
            while h < hashes && b.get(k) == Some(&b'#') {
                h += 1;
                k += 1;
            }
            if h == hashes {
                return Some(k);
            }
        }
        j += 1;
    }
    Some(b.len())
}

fn is_ident_start(b: &[u8], i: usize) -> bool {
    b.get(i)
        .map(|&c| c.is_ascii_alphabetic() || c == b'_' || c >= 0x80)
        .unwrap_or(false)
}

fn is_ident_continue(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn scan_ident(b: &[u8], i: usize) -> (Kind, usize) {
    // Raw identifier `r#fn`: one Ident token whose text is "r#fn".
    if b[i] == b'r'
        && b.get(i + 1) == Some(&b'#')
        && b.get(i + 2).map(|&c| is_ident_continue(c)).unwrap_or(false)
    {
        let mut j = i + 2;
        while j < b.len() && is_ident_continue(b[j]) {
            j += 1;
        }
        return (Kind::Ident, j);
    }
    let mut j = i;
    while j < b.len() && is_ident_continue(b[j]) {
        j += 1;
    }
    (Kind::Ident, j)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idents(src: &str) -> Vec<String> {
        tokenize(src)
            .into_iter()
            .filter(|t| t.kind == Kind::Ident)
            .map(|t| t.text(src).to_string())
            .collect()
    }

    #[test]
    fn skips_comments_and_strings() {
        let src = r##"
            // fn fake() -> c: i32 = {}
            /* fn also_fake() -> x: i32 { } */
            let s = "fn in_string() -> c: i32 = {}";
            let r = r#"fn raw() -> c: i32 = {}"#;
            fn real() {}
        "##;
        assert_eq!(idents(src), vec!["let", "s", "let", "r", "fn", "real"]);
    }

    #[test]
    fn lifetimes_are_not_char_literals() {
        let src = "fn f<'a>(x: &'a str) -> &'a str { x }";
        assert_eq!(idents(src), vec!["fn", "f", "x", "str", "str", "x"]);
        assert!(tokenize(src).iter().any(|t| t.kind == Kind::FatArrow));
    }

    #[test]
    fn char_literals_are_skipped() {
        let src = "fn f() { let c = '\''; let d = 'x'; let e = '\\'; }";
        assert_eq!(idents(src), vec!["fn", "f", "let", "c", "let", "d", "let", "e"]);
    }

    #[test]
    fn pipes_are_tokenized() {
        let src = "a | b || c";
        let kinds: Vec<Kind> = tokenize(src).iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![Kind::Ident, Kind::Pipe, Kind::Ident, Kind::PipePipe, Kind::Ident]
        );
    }

    #[test]
    fn colon_and_paths_are_distinguished() {
        let src = "a: b c::d";
        let toks = tokenize(src);
        assert_eq!(toks[1].kind, Kind::Colon);
        assert_eq!(toks[4].kind, Kind::ColonColon);
    }
}
