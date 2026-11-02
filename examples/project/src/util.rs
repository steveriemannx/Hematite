// Plain Rust module: it uses no Hematite syntax at all.
// `hm build` copies it into the generated crate byte for byte.
pub fn repeated(text: &str, times: usize) -> String {
    text.repeat(times)
}

pub fn shout(text: &str) -> String {
    format!("{}!", text.to_uppercase())
}
