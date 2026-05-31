# whatis (fork of hyperpolyglot)

Programming-language detector written in Rust, ported from GitHub Linguist (Ruby). This fork lives at `git@github.com:dzli/whatis.git`. Upstream `monkslc/hyperpolyglot` is unmaintained — file PRs against this fork or Linguist itself, not upstream.

The canonical Ruby reference implementation is checked out at `~/tests/linguist`. Consult it when reasoning about classifier/tokenizer behavior — it's significantly more sophisticated than this Rust port in places.

## Binaries

| bin | source | purpose |
|---|---|---|
| `hyply` | `src/bin/main.rs` | Original CLI. Reports aggregate language makeup of a directory (`85% Rust, 15% RenderScript`). Filters out `Data`/`Prose` language types. |
| `whatis` | `src/bin/whatis.rs` | Per-file detection added in this fork. Shows every file's detected language including Data/Prose. Strategy flags `-f/-e/-s/-r/-c` toggle filename/extension/shebang/heuristics/classifier. `--tficf` selects the new TF-ICF classifier instead of naive Bayes. No flags = full default pipeline. |
| `codegen` | `src/bin/codegen.rs` | Regenerates everything under `src/codegen/` from `languages.yml`, `heuristics.yml`, and `samples/`. **Run this whenever you change samples or YAML config.** |

## Specializations (layered detection)

`src/detectors/specializations.rs` runs after the classifier (and after heuristics/extension paths) to upgrade certain detections. The idea: variants whose distinguishing feature is a **marker** (regex on file head) belong in a layer above the classifier, not as sibling centroids. Modelling them as flat centroids causes centroid overlap — the variant's training corpus is structurally a superset of its base (e.g. "HTML + extras"), so real base-language files get pulled toward the variant by cosine similarity.

Two kinds of rules:

* **Base-conditional** — only fire when the prior stage chose a specific base. Use when the marker could plausibly appear in unrelated content (string literals, comments). The classifier verdict is the safety net.
* **Marker-authoritative** — fire regardless of prior stage because the marker at offset 0 is unique to the variant (no normal file would naturally produce it). Effectively a miniature magic-byte stage layered on top of the classifier.

Currently active:

| Variant | Rule kind | Marker |
|---|---|---|
| `HTA` (specialization of HTML) | base-conditional on HTML | `(?i)<HTA:APPLICATION\b` |
| `VBScript+HTMLDecoy` | marker-authoritative | `(?i)^\s*'<!DOCTYPE\s+html` |

Specialized variants keep their entry in `languages.yml` (for the `Language` info struct, color, id, etc.) but their `samples/<Variant>/` directory is **deleted** — they have no classifier centroid. The specialization rule is the only path back to the variant.

Rule of thumb: **if a variant's distinguishing feature is a regex marker, layer it. If it's a token *distribution* shape (JSFuck brackets, LongIdent 2000-char names, CharCodeSubtract arithmetic, JSONPacked structure, URIEncoded body), keep it as a flat centroid.**

## Two classifiers

Both are wired into the same detection pipeline; the difference is which one fires when extension/shebang/heuristics fail to disambiguate:

1. **Naive Bayes** (default, `src/detectors/classifier.rs`). Per-token MLE log probabilities. Uses `polyglot_tokenizer::get_key_tokens` — single-char symbols, raw identifiers, comments dropped. Model lives in `src/codegen/token-log-probabilities.rs` (~8 MB).

2. **TF-ICF cosine similarity** (`--tficf`, `src/detectors/tficf_classifier.rs`). Mirrors Linguist's classifier: TF-ICF weighting (downweights tokens shared across many languages), L2-normalized per-language centroids, cosine similarity, `MIN_DOCUMENT_FREQUENCY=2` vocabulary filter. Uses `polyglot_tokenizer::get_linguist_tokens` — typed `COMMENT<opener>` tokens, `SHEBANG#!<interp>` synthesized, sigil-prefixed idents (`@var`, `$var`, `.bar`), multi-char operator coalescing. Model in `src/codegen/tficf-model.rs` (~5 MB).

Held-out content-only accuracy on 19 languages (last commit): Bayes 14/19, TF-ICF **18/19**. TF-ICF is roughly always equal or better on realistic content; the gap is largest on Markdown, Shell, TypeScript, PHP, JSON.

For deep background on what was deliberately ported vs dropped from Linguist, see `~/.claude/projects/-home-dzli-tests-hyperpolyglot/memory/classifier-gaps-vs-linguist.md`.

## Build / test / run

```bash
cargo build --release --bin whatis           # primary CLI for this fork
cargo build --release --bin hyply            # original aggregate CLI
cargo run   --release --bin codegen          # retrain both classifier models
cargo test  --release --lib                  # library tests (43 should pass)
cargo test  --release -p polyglot_tokenizer  # tokenizer tests
```

`whatis` smoke commands:

```bash
whatis                       # walk cwd, default pipeline (Bayes when classifier fires)
whatis <dir>                 # walk a directory
whatis -c <file>             # force content-only classification
whatis -c --tficf <file>     # force TF-ICF
whatis -e -r <dir>           # extension + heuristics only, no classifier fallback
```

## Layout

```
src/
  lib.rs                       core detect()/get_language_breakdown() + Language types
  bin/
    main.rs                    hyply
    whatis.rs                  per-file CLI with strategy + classifier toggles
    codegen.rs                 trainer; emits everything in src/codegen/
  detectors/
    classifier.rs              naive Bayes scorer
    tficf_classifier.rs        TF-ICF cosine-similarity scorer
    extensions.rs, filenames.rs, heuristics.rs, interpreters.rs
  codegen/                     ALL files here are generated; do not hand-edit
    token-log-probabilities.rs  naive Bayes model (~8 MB)
    tficf-model.rs              TF-ICF model (~5 MB)
    extension-language-map.rs
    filename-language-map.rs
    interpreter-language-map.rs
    languages.rs
    language-info-map.rs
    disambiguation-heuristics-map.rs
  filters/
    documentation.rs, vendor.rs  Linguist-style exclusion lists

crates/polyglot_tokenizer/
  src/tokenizer.rs             hand-rolled lexer (Token enum; has pre-existing bugs around block comments)
  src/lib.rs                   get_key_tokens (old) and re-export of get_linguist_tokens
  src/linguist_tokens.rs       Linguist-style post-processor: typed COMMENT/SHEBANG, multi-char ops, sigil idents

languages.yml, heuristics.yml, documentation.yml, vendor.yml   shared with Linguist; serve as codegen input
samples/                       per-language training corpora; subdirectory name = label
```

## Adding a new language / training data

1. Drop sample files into `samples/<LanguageName>/`. Subdirectory name is the class label (one quirk: dir `Fstar` maps to `F*`).
2. If the language isn't already in `languages.yml`, add an entry with extension/filename/interpreter info — otherwise extension matching won't route to it.
3. Run `cargo run --release --bin codegen` — this retrains **both** classifier models from scratch, ~10s.
4. Run `cargo build --release --bin whatis` to pick up the regenerated artifacts.
5. Smoke-test with `whatis -c` on a held-out file.

No `build.rs` triggers codegen automatically — you must run it manually after any sample/yaml change.

## Recurring traps

- **HashMap iteration is non-deterministic** in `codegen.rs`, so running codegen rewrites `extension-language-map.rs`, `interpreter-language-map.rs`, and `languages.rs` with reordered entries even when nothing else changed. Treat as noise; the content is functionally identical.
- **Block-comment delimiters are broken** in the base tokenizer (`/* */`, `<!-- -->`, `{- -}`, `(* *)`). 5 pre-existing tokenizer tests fail because of this. Consequence: `COMMENT/*`, `COMMENT<!--` etc. are rarely emitted by the linguist tokenizer. Line comments work fine.
- **`#var` sigil combining is impossible** because the base tokenizer commits `#` to line-comment parsing. `@var`, `$var`, `.bar` work.
- **`MAX_TOKEN_BYTES = 32`** filters out tokens longer than 32 bytes at both training and classification time. Brackets `()[]{}` are intentionally never coalesced in the linguist tokenizer — without this exception, JSFuck-style bracket-heavy input would collapse into one giant token that the cap silently discards.
- **`whatis` takes only one PATH argument** (not multiple). Glob/expand in the shell instead.
- **`OPENER<…>` synthetic tokens** are emitted 100 times when the file starts with one of these markers at offset 0 (after an optional BOM). The repetition saturates `TFICF_TF_CAP = 100` for TF-ICF and dominates the per-token log-prob sum for Bayes. Both `get_linguist_tokens` (TF-ICF) and `get_key_tokens` (Bayes) share the same `detect_opener()` and `OPENER_EMIT_COUNT`.

  | Marker (case-insensitive parts noted) | Pseudo-token | Anchors detection of |
  |---|---|---|
  | `<?php` | `OPENER<?php` | PHP |
  | `<?hh` | `OPENER<?hh` | Hack |
  | `<?xml` | `OPENER<?xml` | XML |
  | `<script` then `vbscript` in next 64 chars (case-ins.) | `OPENER<script>VBScript` | VBScript |
  | `<!DOCTYPE html` (case-ins.) | `OPENER<!DOCTYPE>html` | HTML |
  | `<html` followed by `>` / space / newline / EOF (case-ins.) | `OPENER<html>` | HTML |

## TODO / future improvements

- **Per-token predefined weights instead of repeated emission.** Today `get_linguist_tokens` emits each `OPENER<?…>` pseudo-token 100× to saturate `TFICF_TF_CAP` and dominate L2 normalization. It works but is a hack: it wastes Vec capacity and bakes the boost into the tokenizer rather than the classifier. A cleaner design: have the tokenizer emit OPENER once, and let the TF-ICF trainer/scorer multiply selected tokens by a configurable per-token weight (e.g. `OPENER_WEIGHT_MULTIPLIER = 100`) at TF computation time. Same effect, no repetition, easier to tune per-marker. Same applies if/when we add other anchored-position pseudo-tokens (`OPENER<%@`, `OPENER<!DOCTYPE`, magic-byte markers, etc.).
- **Modeline / magic-byte stage in the detection pipeline.** Linguist Ruby runs `Strategy::Modeline` before the classifier on every file regardless of extension, catching unambiguous openers (`<?php`, `<?xml`, shebangs, vim/emacs modelines). The Rust port has no equivalent; extension-less files with strong openers currently rely on the classifier (now propped up by `OPENER<?…>` pseudo-tokens — see above). A proper modeline strategy would obsolete that hack for these cases.

## Reference: project memory

Deeper background lives in `~/.claude/projects/-home-dzli-tests-hyperpolyglot/memory/`:

- `linguist-upstream.md` — paths into the Ruby reference and what to consult where
- `hyperpolyglot-inactive.md` — fork/maintenance status and origin remote
- `classifier-gaps-vs-linguist.md` — 12-item list of Linguist features not (yet) in this Rust port, ordered by impact
