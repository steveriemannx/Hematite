mod lexer;
mod transpile;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
hm - Hematite: a cpp2-flavoured syntax layer for Rust

Usage:
  hm expand <file.hm> [-o out.rs]   lower only; print or write the Rust (single file)
  hm expand <crate-dir>             lower every .hm in a crate into target/hematite/<name>/
  hm build  <file.hm | crate-dir>   lower + compile; a directory must contain Cargo.toml
  hm run    <file.hm | crate-dir>   lower + compile + run
  hm --version | --help

Syntax:
  fn f(a: i32) -> name: T [=] { body }
  * `name:` and `=` are signature-only declarations; both are erased when lowering
  * the body is 100% normal Rust (`return` / a tail expression / nothing for unit)

Crate mode:
  hm build examples/project          # mirror the crate into target/hematite/<name>/,
                                     # turn .hm into .rs, copy the rest, then run cargo
  * target/ and .git/ inside the directory are skipped
  * the generated crate keeps source line numbers, so diagnostics map back to .hm
";

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let cmd = args.remove(0);
    let res = match cmd.as_str() {
        "expand" => cmd_expand(&args),
        "build" => cmd_build(&args, false),
        "run" => cmd_build(&args, true),
        "help" | "-h" | "--help" => {
            println!("{USAGE}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("hm {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => {
            eprintln!("hm: unknown command `{other}`\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("hm: {msg}");
            ExitCode::from(1)
        }
    }
}

// ---------------------------------------------------------------- expand

fn cmd_expand(args: &[String]) -> Result<(), String> {
    let mut input: Option<&str> = None;
    let mut out_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--out" => {
                i += 1;
                out_path = Some(args.get(i).ok_or("`-o` requires an output path")?);
            }
            s if s.starts_with('-') => return Err(format!("unknown option `{s}`")),
            s => input = Some(s),
        }
        i += 1;
    }
    let input = input.ok_or("missing input file (.hm) or crate directory")?;
    if Path::new(input).is_dir() {
        if out_path.is_some() {
            return Err("`-o` only applies to a single file".into());
        }
        return expand_crate(Path::new(input));
    }
    let exp = expand_file(input)?;
    match out_path {
        Some(p) => fs::write(p, &exp.output).map_err(|e| format!("failed to write `{p}`: {e}"))?,
        None => print!("{}", exp.output),
    }
    Ok(())
}

fn expand_file(input: &str) -> Result<transpile::Expansion, String> {
    let src = fs::read_to_string(input).map_err(|e| format!("failed to read `{input}`: {e}"))?;
    let exp = transpile::expand(&src).map_err(|e| {
        let (l, c) = e.line_col(&src);
        format!("{input}:{l}:{c}: {}", e.message)
    })?;
    emit_warnings(&exp, input);
    Ok(exp)
}

// ---------------------------------------------------------------- build / run

fn cmd_build(args: &[String], run: bool) -> Result<(), String> {
    let input = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .ok_or("missing input (.hm file or directory containing Cargo.toml)")?;
    let path = Path::new(input);
    if path.is_dir() {
        build_crate(path, run)
    } else {
        build_file(input, run)
    }
}

fn build_file(input: &str, run: bool) -> Result<(), String> {
    let exp = expand_file(input)?;
    fs::create_dir_all("target/hematite")
        .map_err(|e| format!("failed to create target/hematite: {e}"))?;
    let stem = Path::new(input)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot get a file stem from `{input}`"))?;
    let rs_path = format!("target/hematite/{stem}.rs");
    let bin_path = format!("target/hematite/{stem}");
    fs::write(&rs_path, &exp.output).map_err(|e| format!("failed to write `{rs_path}`: {e}"))?;

    let mappings = vec![MappedFile {
        generated: rs_path.clone(),
        original: input.to_string(),
        expansion: Some(0),
    }];
    let expansions = vec![exp];

    let output = Command::new("rustc")
        .args(["--edition", "2021", &rs_path, "-o", &bin_path])
        .output()
        .map_err(|e| format!("failed to run rustc: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{}", remap_diagnostics(&stderr, &mappings, &expansions));
    if !output.status.success() {
        return Err(format!(
            "compilation failed (the generated Rust is in {rs_path})"
        ));
    }

    if run {
        let status = Command::new(&bin_path)
            .status()
            .map_err(|e| format!("failed to run `{bin_path}`: {e}"))?;
        if !status.success() {
            return Err(format!("program exited with code {:?}", status.code()));
        }
    } else {
        println!("hm: ok  {bin_path}");
    }
    Ok(())
}

/// A crate that has been lowered into `target/hematite/<name>/`.
struct PreparedCrate {
    name: String,
    out_root: PathBuf,
    mappings: Vec<MappedFile>,
    expansions: Vec<transpile::Expansion>,
}

/// Mirror a crate: lower every `.hm` to `.rs` and copy everything else.
fn prepare_crate(dir: &Path) -> Result<PreparedCrate, String> {
    if !dir.join("Cargo.toml").is_file() {
        return Err(format!(
            "`{}` is not a Hematite crate (no Cargo.toml); pass a single .hm file instead",
            dir.display()
        ));
    }
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("cannot get a directory name from `{}`", dir.display()))?
        .to_string();
    let out_root = PathBuf::from("target/hematite").join(&name);
    if out_root.exists() {
        fs::remove_dir_all(&out_root)
            .map_err(|e| format!("failed to clean `{}`: {e}", out_root.display()))?;
    }
    fs::create_dir_all(&out_root)
        .map_err(|e| format!("failed to create `{}`: {e}", out_root.display()))?;

    let mut mappings: Vec<MappedFile> = Vec::new();
    let mut expansions: Vec<transpile::Expansion> = Vec::new();
    mirror(dir, Path::new(""), &out_root, &mut mappings, &mut expansions)?;
    Ok(PreparedCrate {
        name,
        out_root,
        mappings,
        expansions,
    })
}

/// `hm expand <crate-dir>`: generate all `.rs` files without compiling.
fn expand_crate(dir: &Path) -> Result<(), String> {
    let prepared = prepare_crate(dir)?;
    let mut generated: Vec<&str> = prepared
        .mappings
        .iter()
        .filter(|m| m.expansion.is_some())
        .map(|m| m.generated.as_str())
        .collect();
    generated.sort();

    println!(
        "hm: generated {} .rs file(s) in {}",
        generated.len(),
        prepared.out_root.display()
    );
    for path in generated {
        println!("  {}", prepared.out_root.join(path).display());
    }
    Ok(())
}

fn build_crate(dir: &Path, run: bool) -> Result<(), String> {
    let PreparedCrate {
        name,
        out_root,
        mappings,
        expansions,
    } = prepare_crate(dir)?;

    let cwd = env::current_dir().map_err(|e| format!("failed to get the current directory: {e}"))?;
    let cargo_target = cwd.join("target/hematite").join(format!("{name}-target"));

    let output = Command::new("cargo")
        .arg("build")
        .current_dir(&out_root)
        .env("CARGO_TARGET_DIR", &cargo_target)
        .output()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprint!("{}", remap_diagnostics(&stderr, &mappings, &expansions));
    if !output.status.success() {
        return Err(format!(
            "cargo build failed (the generated crate is in {})",
            out_root.display()
        ));
    }

    if run {
        let status = match crate_binary(&out_root, &cargo_target) {
            Some(bin) => Command::new(&bin)
                .current_dir(&out_root)
                .status()
                .map_err(|e| format!("failed to run `{}`: {e}", bin.display()))?,
            // No binary found (custom [[bin]], etc.): fall back to cargo run.
            None => Command::new("cargo")
                .args(["run", "--quiet"])
                .current_dir(&out_root)
                .env("CARGO_TARGET_DIR", &cargo_target)
                .status()
                .map_err(|e| format!("failed to run cargo run: {e}"))?,
        };
        if !status.success() {
            return Err(format!("program exited with code {:?}", status.code()));
        }
    } else {
        println!("hm: ok  {} (cargo build)", out_root.display());
    }
    Ok(())
}

/// Locate the executable produced by `cargo build` (default: `[package] name`).
fn crate_binary(out_root: &Path, cargo_target: &Path) -> Option<PathBuf> {
    let manifest = fs::read_to_string(out_root.join("Cargo.toml")).ok()?;
    let pkg = parse_package_name(&manifest).or_else(|| {
        out_root
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
    })?;
    let debug = cargo_target.join("debug");
    for candidate in [debug.join(&pkg), debug.join(format!("{pkg}.exe"))] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn parse_package_name(manifest: &str) -> Option<String> {
    let mut in_package = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if !in_package {
            continue;
        }
        let Some(rest) = line.strip_prefix("name") else {
            continue;
        };
        let Some(value) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Mirror the project under `src_root` into `out_root`: `.hm` files are lowered to
/// `.rs` files with the same stem, everything else is copied as-is; `target/` and
/// `.git/` are skipped.
fn mirror(
    src_root: &Path,
    rel: &Path,
    out_root: &Path,
    mappings: &mut Vec<MappedFile>,
    expansions: &mut Vec<transpile::Expansion>,
) -> Result<(), String> {
    let src_dir = src_root.join(rel);
    let dst_dir = out_root.join(rel);
    fs::create_dir_all(&dst_dir)
        .map_err(|e| format!("failed to create `{}`: {e}", dst_dir.display()))?;

    let entries = fs::read_dir(&src_dir)
        .map_err(|e| format!("failed to read directory `{}`: {e}", src_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("failed to read a directory entry: {e}"))?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name == "target" || name == ".git" {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|e| format!("failed to get the file type: {e}"))?;
        let child_rel = rel.join(&file_name);
        let src_path = src_root.join(&child_rel);

        if file_type.is_dir() {
            mirror(src_root, &child_rel, out_root, mappings, expansions)?;
            continue;
        }
        if !file_type.is_file() {
            continue; // symlinks and the like
        }

        let is_hm = child_rel.extension().and_then(|s| s.to_str()) == Some("hm");
        if is_hm {
            let display = src_path.display().to_string();
            let src = fs::read_to_string(&src_path)
                .map_err(|e| format!("failed to read `{display}`: {e}"))?;
            let exp = transpile::expand(&src).map_err(|e| {
                let (l, c) = e.line_col(&src);
                format!("{display}:{l}:{c}: {}", e.message)
            })?;
            emit_warnings(&exp, &display);
            let rs_rel = child_rel.with_extension("rs");
            let dst_path = out_root.join(&rs_rel);
            if dst_path.exists() {
                return Err(format!(
                    "`{}` conflicts with a sibling `.rs` file: a directory cannot contain both `foo.hm` and `foo.rs`",
                    display
                ));
            }
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create `{}`: {e}", parent.display()))?;
            }
            fs::write(&dst_path, &exp.output)
                .map_err(|e| format!("failed to write `{}`: {e}", dst_path.display()))?;
            let idx = expansions.len();
            expansions.push(exp);
            mappings.push(MappedFile {
                generated: path_to_diag(&rs_rel),
                original: display,
                expansion: Some(idx),
            });
        } else {
            let dst_path = out_root.join(&child_rel);
            if dst_path.exists() {
                return Err(format!(
                    "`{}` conflicts with a generated file: a directory cannot contain both `foo.hm` and `foo.rs`",
                    src_path.display()
                ));
            }
            fs::copy(&src_path, &dst_path).map_err(|e| {
                format!(
                    "failed to copy `{}` -> `{}`: {e}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
            mappings.push(MappedFile {
                generated: path_to_diag(&child_rel),
                original: src_path.display().to_string(),
                expansion: None,
            });
        }
    }
    Ok(())
}

fn path_to_diag(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

// ---------------------------------------------------------------- diagnostics

struct MappedFile {
    /// Path of the generated file under the **working directory** (the one that
    /// shows up in rustc/cargo diagnostics).
    generated: String,
    /// Path of the corresponding original `.hm` / source file.
    original: String,
    /// `Some(i)` selects expansion `i` for line/column compensation.
    expansion: Option<usize>,
}

/// Replace `generated:line:col` in diagnostic text with `original:line:col`.
fn remap_diagnostics(
    text: &str,
    mappings: &[MappedFile],
    expansions: &[transpile::Expansion],
) -> String {
    let mut order: Vec<&MappedFile> = mappings.iter().collect();
    order.sort_by_key(|m| std::cmp::Reverse(m.generated.len()));

    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let mut matched = false;
        for m in &order {
            let Some(rest) = text[i..].strip_prefix(m.generated.as_str()) else {
                continue;
            };
            let Some((line, col, consumed)) = parse_two_numbers(rest) else {
                continue;
            };
            let (ol, oc) = match m.expansion {
                Some(idx) => expansions[idx].map_line_col(line, col),
                None => (line, col),
            };
            out.push_str(&m.original);
            out.push_str(&format!(":{ol}:{oc}"));
            i += m.generated.len() + consumed;
            matched = true;
            break;
        }
        if matched {
            continue;
        }
        let ch = text[i..].chars().next().expect("i is a char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn parse_two_numbers(s: &str) -> Option<(usize, usize, usize)> {
    let b = s.as_bytes();
    if b.first() != Some(&b':') {
        return None;
    }
    let mut j = 1;
    let mut line = 0usize;
    let mut n = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        line = line * 10 + (b[j] - b'0') as usize;
        j += 1;
        n += 1;
    }
    if n == 0 || b.get(j) != Some(&b':') {
        return None;
    }
    j += 1;
    let mut col = 0usize;
    let mut n2 = 0;
    while j < b.len() && b[j].is_ascii_digit() {
        col = col * 10 + (b[j] - b'0') as usize;
        j += 1;
        n2 += 1;
    }
    if n2 == 0 {
        return None;
    }
    Some((line, col, j))
}

fn emit_warnings(exp: &transpile::Expansion, input: &str) {
    for w in &exp.warnings {
        let (l, c) = exp.orig_line_col(w.offset);
        eprintln!("hm: warning: {input}:{l}:{c}: {}", w.message);
    }
}
