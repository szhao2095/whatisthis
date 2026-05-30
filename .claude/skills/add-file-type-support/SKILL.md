---
name: add-file-type-support
description: Improve `whatis` detection for a file type or obfuscation variant. Use when the user provides a directory whose files should all be classified as one (sub)type but `whatis -c --tficf` is mis-detecting some, or when the user wants to add an entirely new variant to the classifier.
---

# Add or improve file-type detection in whatis

You are working in the `whatis` repo (a fork of hyperpolyglot). Follow this playbook to extend or improve detection of a file type or one of its obfuscation variants.

## Inputs & assumptions

- The user provides a **directory of files**. Every file in it should belong to **one** expected base type (e.g., JavaScript, Python, Shell), possibly across several obfuscation variants of that base.
- The expected base type is usually derivable from the directory name (`.../JS/`, `.../PY/`). If you cannot tell, **ask the user before doing anything else.**
- Variants follow Linguist's `Base+Technique` convention (`JavaScript+JSFuck`, `JavaScript+ObfuscatorIO`, etc.). Each obfuscation **technique** gets its own variant so its token distribution does not pollute the base centroid. Different *configurations* of the same technique (e.g., obfuscator.io with a different identifier mangler) stay in the same variant — they share the structural signature.

## Step 1 — Baseline scan

Run the classifier with both heuristics and content-only classifier on the user-provided directory:

```bash
./target/release/whatis -r -c --tficf <dir>
```

If `target/release/whatis` doesn't exist or is stale, rebuild first:

```bash
cargo build --release --bin whatis
```

Record which files map to which detected language. This is the baseline.

## Step 2 — Categorise and propose

For every mis-classified file, decide which bucket it belongs to:

1. **Base type** (no variant needed) — file is plain, classifier is just confused. Cause is usually weak base-corpus coverage of that sub-style.
2. **An existing variant** — file matches a known technique already in `samples/Base+Variant/`. Just need more training samples in that bucket.
3. **A new variant** — file uses a technique not currently modelled. Need a new `Base+Variant` class.

How to decide between (2) and (3): inspect the actual token shapes (long identifiers, hex literals, `\x..` escapes, `String.fromCharCode(...)`, `eval(atob(...))`, etc.) and compare to the existing variant samples in `samples/Base+*`. Cluster files that share the same structural signature. If the signature matches an existing variant *technique*, it's (2); if it's a different decoder/encoder altogether, it's (3).

Surface the decision to the user as a table:

| Group | Files | Current detection | Decision |
|---|---|---|---|
| A. <short signature> | <count> | <detected as> | NEW variant `Base+Foo` |
| B. <short signature> | <count> | <detected as> | EXTEND `Base+Bar` |
| C. <short signature> | <count> | <detected as> | base type (already correct) |

Also report:
- **How many unique files per bucket** after md5-dedup (`md5 -r <files>`). User policy expects ≥10 representative samples per variant; if a bucket has fewer, say so and ask whether to (a) generate more synthetic siblings, or (b) ask the user for more real samples.
- **Naming**: propose specific variant names. Use `Base+Technique` (PascalCase technique, no spaces).
- **Heuristics opportunity**: if any bucket has a tight structural signature (a regex that matches the head/tail reliably), call it out — heuristics can be a useful belt-and-suspenders alongside the classifier.

**Wait for explicit user confirmation** before any file writes or codegen. Confirmation items to lock in:
- Variant names
- Whether to extend an existing variant or split into a new one
- Whether to generate synthetic samples or request originals
- Whether to also augment heuristics

## Step 3 — Generate training samples and retrain

### Sample policy

- **Do NOT copy the user's original files into `samples/`.** Original files may carry privacy/security risk of unknown provenance.
- Default to **synthetic** samples that faithfully reproduce the *structural signature* of the originals (token shape, file size order of magnitude, control structure) but with randomized identifiers/constants/payloads. See `templates/gen_js_variants.py` for a working starting point.
- If a structure is genuinely too hard to reproduce synthetically, **ask the user** for explicit permission to add originals (or sanitised originals) before doing so.
- Target ~10–15 synthetic files per new variant. Vary file size, identifier names, payload lengths, and constants so the centroid isn't peaked on one trivially-distinguishable shape.

### File layout

```
samples/Base+Variant/<descriptive_name>_<n>.<ext>
```

Subdirectory name = class label. The base tokenizer's `MAX_TOKEN_BYTES = 32` and TF-ICF's `TF_CAP = 100` mean obscenely-large files don't help — keep samples under ~1 MB each.

### Register the variant in languages.yml

Add an entry near the other `Base+*` entries:

```yaml
Base+Variant:
  type: programming
  tm_scope: source.<base>
  group: Base
  color: "#<base-color>"
  extensions:
  - ".<ext>"
  language_id: <next free, see below>
```

Find a fresh `language_id` by `grep "language_id: 900" languages.yml | sort -t: -k2 -n | tail`. Existing convention: JS+ variants in `900200–900206`, VBA+ in `900300+`. Pick an unused number grouped by base language.

**Extension claims have side-effects.** Extension routing limits the candidate set the classifier sees. Implications:

- If you claim a common extension (e.g., `.json`, `.txt`, `.xml`), your variant becomes a candidate for **every** file of that extension. The Bayes classifier in particular can pick your variant for unrelated files of that extension — this WILL break the `test_detect_accuracy` library test.
- Prefer to either (a) claim the base language's extensions (e.g., `.vba` for a VBA+ variant) so routing stays within that base, or (b) claim no extensions at all and rely on the classifier alone for extension-less files.
- If your training samples have an extension that the variant doesn't claim, the test will flag them as misclassified by `Extension(<otherlang>)`. Either claim that extension or rename the samples.

### Watch out for fallback heuristics that shortcut the classifier

Some base extensions have a no-pattern fallback rule in `heuristics.yml` (e.g., `.vba` → `VBA`). The heuristics stage runs **before** the classifier, so a fallback like that will catch every file of that extension and return the base language — your variant samples never reach the classifier and the `test_detect_accuracy` test fails with `Heuristics("Base")` errors.

Fix: add a more-specific rule for your variant BEFORE the fallback. Example for VBA+JSONPacked under `.vba`:

```yaml
- extensions: ['.vba']
  rules:
  - language: Vim script
    pattern: '^UseVimball'
  - language: VBA+JSONPacked
    pattern: '^\s*\{\s*"[^"]+"\s*:\s*"Attribute\s+VB_Name\s*='
  - language: VBA   # fallback stays last
```

Heuristic patterns are PCRE2 against the file head. Prefer anchored patterns (`^...`) and make them specific enough that base-language files don't match. If you can't write a clean structural pattern for the variant, the fallback alternative is to give your training samples no extension at all — that bypasses the extension router and heuristic stage entirely, letting the classifier decide and the accuracy test pass.

### Retrain and rebuild

```bash
cargo run --release --bin codegen      # ~10s — rewrites src/codegen/*
cargo build --release --bin whatis     # picks up regenerated model
```

Codegen will rewrite several `src/codegen/*.rs` files. Some of those rewrites contain **noise from non-deterministic HashMap iteration** (extension-language-map, interpreter-language-map, languages.rs) — that's expected and harmless.

### Sanity-check before testing

```bash
cargo test --release --lib             # 43 tests should pass
```

Self-classify every training sample for the touched variants to confirm none leak out of their own bucket:

```bash
for v in samples/Base+Variant samples/Base+OtherTouched; do
  echo "[$v]"
  for f in "$v"/*; do
    [ -f "$f" ] || continue
    out=$(./target/release/whatis -c --tficf "$f" 2>/dev/null | awk -F': ' '{print $2}' | awk -F' \\[' '{print $1}')
    printf "  %-40s -> %s\n" "$(basename "$f")" "$out"
  done
done
```

A few leaks in the base bucket are usually pre-existing (e.g., `dude.js` → TypeScript) and not caused by these changes; verify by spot-checking before assuming regression.

## Step 4 — Verify on the user directory and iterate

Re-run the baseline command and check that **every** file lands on the expected base type or one of its variants:

```bash
./target/release/whatis -r -c --tficf <dir>
```

If any file is still mis-classified, **loop back to Step 2** with just the remaining stragglers. Do not loop more than ~3 times without re-confirming the plan with the user — repeated loops usually mean a wrong variant split or under-trained variant.

## Step 5 — Summary

Tell the user, in a short message:

- Final detection result on the user directory (target: 100% on base + its variants)
- Variants added / extended, with sample counts
- Files touched (`languages.yml`, regenerated codegen artifacts, new samples)
- Library test result
- Any known leaks or caveats that were observed but deliberately left unaddressed

## Common traps (from `CLAUDE.md`)

- `whatis` accepts only **one** PATH arg — shell-glob into a directory, don't pass multiple files.
- `HashMap` iteration is non-deterministic during codegen — diffs in `extension-language-map.rs` / `interpreter-language-map.rs` / `languages.rs` are noise.
- Block-comment delimiters are broken in the base tokenizer — `COMMENT/*`, `COMMENT<!--` are rarely emitted. Don't design a variant signature around them.
- `MAX_TOKEN_BYTES = 32` truncates long tokens at both training and classify time; long identifiers still produce signal because their 32-byte prefix is still distinctive.
- Bracket tokens `()[]{}` are intentionally never coalesced — JSFuck-like bracket-heavy inputs survive.

## Where to read more

- `CLAUDE.md` — repo overview, build/test commands, layout.
- `~/.claude/projects/-home-dzli-tests-hyperpolyglot/memory/classifier-gaps-vs-linguist.md` — what was deliberately not ported from Linguist.
- `samples/JavaScript+*/` — every existing JS variant, with real or synthetic samples; the cleanest references for what good variant samples look like.
- `templates/gen_js_variants.py` (next to this file) — a working sample generator for three JS obfuscation variants, easy to copy and adapt for new ones.
