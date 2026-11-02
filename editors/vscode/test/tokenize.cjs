// Tokenize the TextMate grammar in Node with vscode-textmate + vscode-oniguruma,
// checking the scopes for `.hm` named returns / the function-body `=` and making
// sure plain Rust code is unaffected.
//
//   npm install
//   npm test
"use strict";

const fs = require("fs");
const path = require("path");
const vsctm = require("vscode-textmate");
const oniguruma = require("vscode-oniguruma");

const GRAMMAR_PATH = path.join(__dirname, "..", "syntaxes", "hematite.tmLanguage.json");

async function main() {
  const wasmPath = path.join(path.dirname(require.resolve("vscode-oniguruma")), "onig.wasm");
  const wasmBin = fs.readFileSync(wasmPath).buffer;
  const onigLib = oniguruma.loadWASM(wasmBin).then(() => ({
    createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
    createOnigString: (s) => new oniguruma.OnigString(s),
  }));

  const grammarJson = JSON.parse(fs.readFileSync(GRAMMAR_PATH, "utf8"));
  const registry = new vsctm.Registry({
    onigLib,
    loadGrammar: async (scopeName) =>
      scopeName === "source.hematite" ? grammarJson : null,
  });
  const grammar = await registry.loadGrammar("source.hematite");
  if (!grammar) throw new Error("could not load the source.hematite grammar");

  /** Tokenize the source line by line: [[{startIndex,endIndex,scopes}...], ...] */
  function tokenizeLines(src) {
    const lines = src.split("\n");
    let ruleStack = vsctm.INITIAL;
    return lines.map((line) => {
      const result = grammar.tokenizeLine(line, ruleStack);
      ruleStack = result.ruleStack;
      return result.tokens;
    });
  }

  /** Find the token covering the first occurrence of `needle` on 1-based `lineNo`. */
  function tokenAt(src, lineNo, needle) {
    const lines = src.split("\n");
    const line = lines[lineNo - 1];
    const idx = line.indexOf(needle);
    if (idx < 0) throw new Error(`line ${lineNo} does not contain ${JSON.stringify(needle)}`);
    const tokens = tokenizeLines(src)[lineNo - 1];
    const tok = tokens.find((t) => t.startIndex <= idx && idx < t.endIndex);
    if (!tok) throw new Error(`no token for ${JSON.stringify(needle)} on line ${lineNo}`);
    return tok;
  }

  let failures = 0;
  function check(name, cond, detail) {
    if (cond) {
      console.log(`  ok   ${name}`);
    } else {
      failures++;
      console.log(`  FAIL ${name}`);
      if (detail) console.log(`       ${detail}`);
    }
  }
  const hasScope = (tok, scope) => tok.scopes.includes(scope);

  // ---------------------------------------------------------------- case 1
  console.log("case 1: named return + `=` + a normal `return` in the body");
  const src1 = [
    "fn add(a: i32, b: i32) -> ret: i32 = {",
    "    return a + b;",
    "}",
  ].join("\n");
  const arrow1 = tokenAt(src1, 1, "->");
  const name1 = tokenAt(src1, 1, "ret");
  const eq1 = tokenAt(src1, 1, "= {");
  const ty1 = tokenAt(src1, 1, "i32 =");
  check("`->` has keyword.operator.arrow.hematite", hasScope(arrow1, "keyword.operator.arrow.hematite"), JSON.stringify(arrow1.scopes));
  check("`ret` has variable.parameter.return-name.hematite", hasScope(name1, "variable.parameter.return-name.hematite"), JSON.stringify(name1.scopes));
  check("`=` has keyword.operator.assignment.fn-body.hematite", hasScope(eq1, "keyword.operator.assignment.fn-body.hematite"), JSON.stringify(eq1.scopes));
  check("the return type i32 is still highlighted as a Rust type", ty1.scopes.some((s) => s.includes("type")), JSON.stringify(ty1.scopes));
  check("`return` in the body is still a Rust keyword", tokenAt(src1, 2, "return").scopes.some((s) => s.includes("keyword")), JSON.stringify(tokenAt(src1, 2, "return").scopes));

  // ---------------------------------------------------------------- case 2
  console.log("case 2: named return without `=`");
  const src2 = "fn mul(a: i32, b: i32) -> r: i32 {";
  const name2 = tokenAt(src2, 1, "r: ");
  const arrow2 = tokenAt(src2, 1, "->");
  check("`r` has the return-name scope", hasScope(name2, "variable.parameter.return-name.hematite"), JSON.stringify(name2.scopes));
  check("`->` has the arrow scope", hasScope(arrow2, "keyword.operator.arrow.hematite"), JSON.stringify(arrow2.scopes));

  // ---------------------------------------------------------------- case 3
  console.log("case 3: plain Rust code is left alone");
  const src3 = "let mut x = 1;\nfn f() -> i32 { 0 }";
  const letTok = tokenAt(src3, 1, "let");
  check("`let` is still a Rust keyword", letTok.scopes.some((s) => s.includes("keyword") || s.includes("storage")), JSON.stringify(letTok.scopes));
  const fnArrow = tokenAt(src3, 2, "->");
  check("a plain return-type `->` is not treated as a named return", !hasScope(fnArrow, "keyword.operator.arrow.hematite"), JSON.stringify(fnArrow.scopes));

  // ---------------------------------------------------------------- case 4
  console.log("case 4: named return + path type + attribute");
  const src4 = "#[inline]\nfn main() -> code: std::process::ExitCode {";
  const name4 = tokenAt(src4, 2, "code");
  check("`code` has the return-name scope", hasScope(name4, "variable.parameter.return-name.hematite"), JSON.stringify(name4.scopes));
  check("`#` is still highlighted as an attribute", tokenAt(src4, 1, "#").scopes.some((s) => s.includes("attribute") || s.includes("meta")), JSON.stringify(tokenAt(src4, 1, "#").scopes));

  // ---------------------------------------------------------------- dump
  console.log("\nscope snapshot (case 1, line 1):");
  for (const t of tokenizeLines(src1)[0]) {
    const text = src1.split("\n")[0].slice(t.startIndex, t.endIndex);
    console.log(`  ${JSON.stringify(text).padEnd(12)} ${t.scopes.filter((s) => s.includes("hematite") || s.includes("rust")).join(" ")}`);
  }

  console.log(failures === 0 ? "\nall checks passed" : `\n${failures} check(s) failed`);
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
