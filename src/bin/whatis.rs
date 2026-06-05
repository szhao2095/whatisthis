use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
    process,
};

use clap::{App, Arg};
use hyperpolyglot::{
    detectors::{
        apply_specialization, byte_stats, classify, classify_linear_scored,
        classify_scored, classify_tficf, classify_tficf_scored, detect_structure,
        filter_to_majority_family, get_extension, get_language_from_filename,
        get_languages_from_extension, get_languages_from_heuristics, get_languages_from_shebang,
    },
    region_engine::{region_profile, FileVerdict},
    Detection,
};
use ignore::WalkBuilder;

const MAX_CONTENT_SIZE_BYTES: usize = 51200;

/// Files larger than this trigger 3-chunk profiling when `--chunked` is set.
/// Below it, a single whole-file verdict is still printed (the chunks would
/// overlap with each other anyway).
const CHUNKED_THRESHOLD: usize = 150 * 1024;

/// Size of each individual chunk read in `--chunked` mode. Matches
/// `MAX_CONTENT_SIZE_BYTES` so each chunk is exactly what the classifier
/// would read in single-verdict mode — no new edge cases inside the pipeline.
const CHUNK_SIZE: usize = MAX_CONTENT_SIZE_BYTES;

#[derive(Copy, Clone)]
struct Strategies {
    filename: bool,
    extension: bool,
    shebang: bool,
    heuristics: bool,
    classifier: bool,
    use_tficf: bool,
    use_linear: bool,
    chunked: bool,
    entropy: bool,
    structure: bool,
    family_first: bool,
    /// When Some(t), return Unknown if best classifier score < t.
    unknown_threshold: Option<f64>,
    /// When > 0.0, return Ambiguous if top two scores differ by less than this.
    ambiguity_margin: f64,
    multilabel: bool,
    window_size: usize,
    stride: usize,
    top_k_labels: usize,
    /// When set, switch chunked output from the default 3-chunk (top/mid/bot)
    /// sampling to *tiling* mode: break the file into consecutive chunks of
    /// this many bytes and print a verdict for each.
    tile_chunk_size: Option<usize>,
}

impl Strategies {
    fn needs_file_read(&self) -> bool {
        self.shebang || self.heuristics || self.classifier || self.entropy || self.structure
    }
}

enum DetectResult {
    Found(Detection),
    Ambiguous(Vec<&'static str>),
    Unknown,
}

fn main() {
    let matches = App::new("whatis")
        .about("Detect file type(s). Choose which strategies run; default is all.")
        .arg(Arg::with_name("PATH").index(1).default_value("."))
        .arg(Arg::with_name("filename").short("f").long("filename").help("Use whole-filename match"))
        .arg(Arg::with_name("extension").short("e").long("extension").help("Use extension match"))
        .arg(Arg::with_name("shebang").short("s").long("shebang").help("Use shebang line"))
        .arg(Arg::with_name("heuristics").short("r").long("heuristics").help("Use regex heuristic rules"))
        .arg(Arg::with_name("classifier").short("c").long("classifier").help("Use Bayesian token classifier"))
        .arg(Arg::with_name("tficf").long("tficf").help("When the classifier strategy fires, use the TF-ICF cosine-similarity classifier instead of the default naive Bayes one"))
        .arg(Arg::with_name("linear").long("linear").help("When the classifier strategy fires, use the char 4-gram linear (TF-ICF cosine) classifier. More robust to obfuscation and encoding noise than the token-based classifiers."))
        .arg(Arg::with_name("chunked").long("chunked").help("Profile mode: for files > 150 KB, classify top / middle / bottom 50 KB chunks separately and print one verdict per chunk. Useful for files with mixed content or padding-decoy obfuscation. Smaller files keep the default single-verdict output."))
        .arg(Arg::with_name("chunk-size").long("chunk-size").value_name("SIZE").takes_value(true).help("With --chunked: tile chunks of SIZE bytes across the *entire* file instead of sampling top/middle/bottom. Catches content at any offset. Suffixes accepted: K, M, G (case-insensitive). Implies --chunked. Example: --chunk-size 16K"))
        .arg(Arg::with_name("entropy").long("entropy").help("Print byte-level statistics (Shannon entropy, printable ratio, null ratio, hex/base64 density) for each file alongside the language verdict"))
        .arg(Arg::with_name("structure").long("structure").help("Check for binary/container magic bytes before text classification. For hard binary formats (ELF, PE, GZIP, PNG) the text classifier is suppressed and the format is reported directly. For advisory formats (PDF, ZIP, OLE2) the structure hit is shown alongside the text verdict."))
        .arg(Arg::with_name("family-first").long("family-first").help("Before running the classifier, filter the candidate language set to those in the plurality family (WebScript, Shell, Systems, etc.) according to taxonomy.yml. Reduces cross-family confusion when the extension is ambiguous."))
        .arg(Arg::with_name("unknown-threshold").long("unknown-threshold").value_name("SCORE").takes_value(true).help("When using --tficf, emit <unknown> instead of a forced label if the best cosine similarity score is below SCORE (0.0-1.0). Default: disabled."))
        .arg(Arg::with_name("ambiguity-margin").long("ambiguity-margin").value_name("DELTA").takes_value(true).help("When using --tficf or the Bayes classifier, emit <ambiguous> if the top two scores differ by less than DELTA. Default: 0.0 (disabled)."))
        .arg(Arg::with_name("multilabel").long("multilabel").help("Run the classifier over overlapping sliding windows and aggregate per-label peak/coverage/persistence scores. Prints multiple labels when more than one language scores above the threshold. Useful for mixed-content or polyglot files."))
        .arg(Arg::with_name("window-size").long("window-size").value_name("SIZE").takes_value(true).help("Window size for --multilabel mode. Suffixes K/M/G accepted. Default: 16K."))
        .arg(Arg::with_name("stride").long("stride").value_name("SIZE").takes_value(true).help("Stride between windows for --multilabel mode. Default: 4K."))
        .arg(Arg::with_name("top-k-labels").long("top-k-labels").value_name("N").takes_value(true).help("Maximum number of labels to show in --multilabel output. Default: 3."))
        .get_matches();

    let any = ["filename", "extension", "shebang", "heuristics", "classifier"]
        .iter()
        .any(|k| matches.is_present(k));
    let use_tficf = matches.is_present("tficf");
    let use_linear = matches.is_present("linear");
    let entropy = matches.is_present("entropy");
    let structure = matches.is_present("structure");
    let family_first = matches.is_present("family-first");
    let unknown_threshold = matches.value_of("unknown-threshold").map(|s| {
        s.parse::<f64>().unwrap_or_else(|_| {
            eprintln!("whatis: invalid --unknown-threshold {:?}: must be a float", s);
            process::exit(2);
        })
    });
    let ambiguity_margin = matches.value_of("ambiguity-margin")
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let multilabel = matches.is_present("multilabel");
    let window_size = matches.value_of("window-size")
        .map(|s| parse_size(s).unwrap_or_else(|e| { eprintln!("whatis: invalid --window-size: {}", e); process::exit(2); }))
        .unwrap_or(16 * 1024);
    let stride = matches.value_of("stride")
        .map(|s| parse_size(s).unwrap_or_else(|e| { eprintln!("whatis: invalid --stride: {}", e); process::exit(2); }))
        .unwrap_or(4 * 1024);
    let top_k_labels = matches.value_of("top-k-labels")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3);
    let tile_chunk_size = matches.value_of("chunk-size").map(|s| {
        parse_size(s).unwrap_or_else(|e| {
            eprintln!("whatis: invalid --chunk-size {:?}: {}", s, e);
            process::exit(2);
        })
    });
    // --chunk-size implies --chunked
    let chunked = matches.is_present("chunked") || tile_chunk_size.is_some();
    let opts = if any {
        Strategies {
            filename: matches.is_present("filename"),
            extension: matches.is_present("extension"),
            shebang: matches.is_present("shebang"),
            heuristics: matches.is_present("heuristics"),
            classifier: matches.is_present("classifier"),
            use_tficf,
            use_linear,
            chunked,
            entropy,
            structure,
            family_first,
            unknown_threshold,
            ambiguity_margin,
            multilabel,
            window_size,
            stride,
            top_k_labels,
            tile_chunk_size,
        }
    } else {
        Strategies {
            filename: true,
            extension: true,
            shebang: true,
            heuristics: true,
            classifier: true,
            use_tficf,
            use_linear,
            chunked,
            entropy,
            structure,
            family_first,
            unknown_threshold,
            ambiguity_margin,
            multilabel,
            window_size,
            stride,
            top_k_labels,
            tile_chunk_size,
        }
    };

    let path_arg = matches.value_of("PATH").unwrap();
    let path = Path::new(path_arg);
    if !path.exists() {
        eprintln!("whatis: path not found: {}", path_arg);
        process::exit(1);
    }

    if path.is_file() {
        print_result(path, &opts);
    } else {
        let walker = WalkBuilder::new(path).build();
        for entry in walker.filter_map(Result::ok) {
            let p = entry.path();
            if p.is_dir() {
                continue;
            }
            print_result(p, &opts);
        }
    }
}

fn print_multilabel_result(path: &Path, opts: &Strategies) {
    // Build the scorer closure that mirrors the single-file classifier choice.
    let use_tficf = opts.use_tficf;
    let use_linear = opts.use_linear;
    let scorer = move |text: &str, candidates: &[&'static str]| -> Vec<(&'static str, f64)> {
        if use_linear {
            classify_linear_scored(text, candidates)
        } else if use_tficf {
            classify_tficf_scored(text, candidates)
        } else {
            classify_scored(text, candidates)
        }
    };

    // Derive candidates from extension (same as single-file path).
    let filename_str = path.file_name().and_then(|f| f.to_str());
    let extension = filename_str.and_then(get_extension);
    let candidates: Vec<&'static str> = extension
        .as_ref()
        .map(|e| get_languages_from_extension(e))
        .unwrap_or_default();

    let result: io::Result<FileVerdict> = (|| {
        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        Ok(region_profile(&buf, opts.window_size, opts.stride, &scorer, &candidates))
    })();

    match result {
        Err(e) => println!("{}: <error: {}>", path.display(), e),
        Ok(verdict) => {
            let k = opts.top_k_labels.max(1);
            let top: Vec<_> = verdict.labels.iter().take(k).collect();
            if top.is_empty() {
                println!("{}: <unknown>", path.display());
            } else if top.len() == 1 {
                println!("{}: {} [Classifier]", path.display(), top[0].0);
            } else {
                println!("{}:", path.display());
                for (lang, score) in &top {
                    println!("  {:.3}  {}", score, lang);
                }
            }
        }
    }
}

fn print_result(path: &Path, opts: &Strategies) {
    // Multilabel region-engine path — runs before the single-verdict path.
    if opts.multilabel {
        print_multilabel_result(path, opts);
        if opts.entropy { print_entropy(path); }
        return;
    }

    // Structure stage: read raw magic bytes before any text processing.
    let structure_advisory: Vec<&'static str> = if opts.structure {
        match read_structure_hits(path) {
            Ok(hits) => {
                let suppressing: Vec<_> = hits.iter().filter(|h| h.suppress_text_classifier).collect();
                if !suppressing.is_empty() {
                    // Binary-only format: skip all text classifiers.
                    let names: Vec<&str> = suppressing.iter().map(|h| h.name).collect();
                    println!("{}: {} [Structure]", path.display(), names.join("+"));
                    if opts.entropy { print_entropy(path); }
                    return;
                }
                // Advisory: annotate alongside text verdict.
                hits.iter().map(|h| h.name).collect()
            }
            Err(e) => {
                println!("{}: <error: {}>", path.display(), e);
                return;
            }
        }
    } else {
        vec![]
    };

    if opts.chunked {
        match print_chunked_result(path, opts) {
            Ok(true) => return,        // chunked output was printed
            Ok(false) => {}            // file below threshold, fall through to single-line
            Err(e) => {
                println!("{}: <error: {}>", path.display(), e);
                return;
            }
        }
    }
    let label = match detect_with(path, opts) {
        Ok(DetectResult::Found(d)) => format!("{} [{}]", d.language(), d.variant()),
        Ok(DetectResult::Ambiguous(cands)) => format!("<ambiguous: {}>", cands.join(", ")),
        Ok(DetectResult::Unknown) => "<unknown>".to_string(),
        Err(e) => format!("<error: {}>", e),
    };

    if structure_advisory.is_empty() {
        println!("{}: {}", path.display(), label);
    } else {
        println!("{}: {} [Structure: {}]", path.display(), label, structure_advisory.join("+"));
    }

    if opts.entropy {
        print_entropy(path);
    }
}

fn read_structure_hits(path: &Path) -> io::Result<Vec<hyperpolyglot::detectors::structure::StructureHit>> {
    let mut file = File::open(path)?;
    let mut buf = [0u8; 32];
    let n = file.read(&mut buf)?;
    Ok(detect_structure(&buf[..n]))
}

fn print_entropy(path: &Path) {
    const READ_BYTES: usize = 51200;
    let result = (|| -> io::Result<()> {
        let mut file = File::open(path)?;
        let mut buf = vec![0u8; READ_BYTES];
        let n = file.read(&mut buf)?;
        buf.truncate(n);
        let s = byte_stats(&buf);
        println!(
            "  entropy: H={:.2} bits  printable={:.2}  null={:.3}  hex={:.2}  b64={:.2}",
            s.entropy, s.printable_ratio, s.null_ratio, s.hex_density, s.base64_density
        );
        Ok(())
    })();
    if let Err(e) = result {
        println!("  entropy: <error: {}>", e);
    }
}

/// Chunked profile output. Two modes:
///
/// * If `opts.tile_chunk_size` is `None`: 3-chunk sample at top / middle /
///   bottom (each `CHUNK_SIZE` bytes). Triggers only for files larger than
///   `CHUNKED_THRESHOLD`; smaller files print single-verdict via fallback.
///
/// * If `opts.tile_chunk_size` is `Some(SIZE)`: tile consecutive `SIZE`-byte
///   chunks across the *entire* file. Covers every offset; useful for files
///   where interesting content might be at any position.
///
/// Returns `Ok(true)` if chunked output was emitted, `Ok(false)` if the
/// file is below the threshold (3-chunk mode only — tiling mode always
/// emits regardless of size), or `Err` if I/O failed.
fn print_chunked_result(path: &Path, opts: &Strategies) -> io::Result<bool> {
    let size = std::fs::metadata(path)?.len() as usize;
    if opts.tile_chunk_size.is_none() && size <= CHUNKED_THRESHOLD {
        return Ok(false);
    }

    // Extension-derived candidate set is the same for every chunk; the
    // file's extension doesn't change as we scan along its length.
    let filename_str = path.file_name().and_then(|f| f.to_str());
    let extension = filename_str.and_then(get_extension);
    let candidates: Vec<&'static str> = extension
        .as_ref()
        .map(|e| get_languages_from_extension(e))
        .unwrap_or_default();

    let mut file = File::open(path)?;

    if let Some(tile_size) = opts.tile_chunk_size {
        // Tiling mode: walk the file in chunks of tile_size bytes.
        println!("{}:", path.display());
        let mut offset = 0usize;
        let mut idx = 0;
        while offset < size {
            let len = tile_size.min(size - offset);
            let content = read_chunk_at(&mut file, offset, len)?;
            let det = classify_chunk(&content, opts, &candidates);
            println!(
                "  [{:>4}] {:>8}..{:<8} ({:>5} B)  {}",
                idx,
                offset,
                offset + len,
                len,
                det
            );
            offset += len;
            idx += 1;
        }
        return Ok(true);
    }

    // Default 3-chunk top / middle / bottom mode.
    let top = read_chunk_at(&mut file, 0, CHUNK_SIZE)?;
    let mid_start = (size - CHUNK_SIZE) / 2;
    let mid = read_chunk_at(&mut file, mid_start, CHUNK_SIZE)?;
    let bot_start = size - CHUNK_SIZE;
    let bot = read_chunk_at(&mut file, bot_start, CHUNK_SIZE)?;

    let top_det = classify_chunk(&top, opts, &candidates);
    let mid_det = classify_chunk(&mid, opts, &candidates);
    let bot_det = classify_chunk(&bot, opts, &candidates);

    println!("{}:", path.display());
    println!("  [top]    {}", top_det);
    println!("  [middle] {}", mid_det);
    println!("  [bottom] {}", bot_det);

    Ok(true)
}

/// Parse a size string like `4096`, `16K`, `1M`, `2G` (case-insensitive).
fn parse_size(s: &str) -> Result<usize, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("empty".to_string());
    }
    let (num_str, mult) = match trimmed.chars().last().unwrap().to_ascii_uppercase() {
        'K' => (&trimmed[..trimmed.len() - 1], 1024usize),
        'M' => (&trimmed[..trimmed.len() - 1], 1024 * 1024),
        'G' => (&trimmed[..trimmed.len() - 1], 1024 * 1024 * 1024),
        c if c.is_ascii_digit() => (trimmed, 1),
        c => return Err(format!("unrecognized suffix {:?}", c)),
    };
    let n: usize = num_str
        .trim()
        .parse()
        .map_err(|e| format!("not a number: {}", e))?;
    if n == 0 {
        return Err("must be > 0".to_string());
    }
    Ok(n.saturating_mul(mult))
}

/// Run the classifier (+ specialization) on a single chunk. Skips the
/// shebang / heuristics stages — those are file-level features that don't
/// generalise to mid-file slices. OPENER and SHEBANG pseudo-tokens *can*
/// still fire inside the tokenizer if the chunk happens to start with the
/// matching bytes (essentially never the case for middle/bottom chunks);
/// the per-chunk false-positive rate is negligible in practice.
fn classify_chunk(content: &str, opts: &Strategies, candidates: &[&'static str]) -> String {
    if !opts.classifier {
        return "<classifier disabled>".to_string();
    }
    let content = truncate_to_char_boundary(content, MAX_CONTENT_SIZE_BYTES);
    let lang = if opts.use_tficf {
        classify_tficf(content, candidates)
    } else {
        classify(content, candidates)
    };
    let lang = apply_specialization(lang, content);
    format!("{} [Classifier]", lang)
}

fn read_chunk_at(file: &mut File, offset: usize, len: usize) -> io::Result<String> {
    file.seek(SeekFrom::Start(offset as u64))?;
    let mut buf = vec![0u8; len];
    let n = file.read(&mut buf)?;
    buf.truncate(n);
    // Lossy UTF-8 decode so non-text chunks (rare for our typical inputs)
    // don't error out the whole profile run.
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn detect_with(path: &Path, opts: &Strategies) -> io::Result<DetectResult> {
    let filename_str = path.file_name().and_then(|f| f.to_str());

    if opts.filename {
        if let Some(name) = filename_str {
            if let Some(lang) = get_language_from_filename(name) {
                return Ok(DetectResult::Found(Detection::Filename(lang)));
            }
        }
    }

    let extension = filename_str.and_then(get_extension);
    let mut candidates: Vec<&'static str> = if opts.extension {
        extension
            .as_ref()
            .map(|e| get_languages_from_extension(e))
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    if opts.extension && candidates.len() == 1 {
        return Ok(DetectResult::Found(Detection::Extension(candidates[0])));
    }

    if !opts.needs_file_read() {
        return Ok(match candidates.len() {
            0 => DetectResult::Unknown,
            1 => DetectResult::Found(Detection::Extension(candidates[0])),
            _ => DetectResult::Ambiguous(candidates),
        });
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let mut last_stage = if opts.extension && !candidates.is_empty() {
        LastStage::Extension
    } else {
        LastStage::None
    };

    if opts.shebang {
        let shebang_langs = get_languages_from_shebang(&mut reader)?;
        if !shebang_langs.is_empty() {
            candidates = filter_candidates(candidates, shebang_langs);
            last_stage = LastStage::Shebang;
            if candidates.len() == 1 {
                return Ok(DetectResult::Found(Detection::Shebang(candidates[0])));
            }
        }
        reader.seek(SeekFrom::Start(0))?;
    }

    let mut content = String::new();
    reader.read_to_string(&mut content)?;
    let content = truncate_to_char_boundary(&content, MAX_CONTENT_SIZE_BYTES);

    if opts.family_first && candidates.len() > 1 {
        candidates = filter_to_majority_family(&candidates);
    }

    if opts.heuristics && candidates.len() > 1 {
        if let Some(ext) = extension.as_ref() {
            let langs = get_languages_from_heuristics(ext, &candidates, content);
            let before = candidates.len();
            candidates = filter_candidates(candidates, langs);
            if candidates.len() != before {
                last_stage = LastStage::Heuristics;
            }
        }
    }

    if opts.classifier && candidates.len() != 1 {
        let scores = if opts.use_linear {
            classify_linear_scored(content, &candidates)
        } else if opts.use_tficf {
            classify_tficf_scored(content, &candidates)
        } else {
            classify_scored(content, &candidates)
        };
        // Ambiguity check: top two scores too close to each other.
        if opts.ambiguity_margin > 0.0 && scores.len() >= 2 {
            let delta = scores[0].1 - scores[1].1;
            if delta.abs() < opts.ambiguity_margin {
                return Ok(DetectResult::Ambiguous(
                    scores[..2.min(scores.len())].iter().map(|(l, _)| *l).collect(),
                ));
            }
        }
        // Unknown threshold: best score is too low.
        if let Some(threshold) = opts.unknown_threshold {
            if scores[0].1 < threshold {
                return Ok(DetectResult::Unknown);
            }
        }
        let lang = apply_specialization(scores[0].0, content);
        return Ok(DetectResult::Found(Detection::Classifier(lang)));
    }

    Ok(match candidates.len() {
        0 => DetectResult::Unknown,
        1 => {
            let lang = apply_specialization(candidates[0], content);
            DetectResult::Found(match last_stage {
                LastStage::Heuristics => Detection::Heuristics(lang),
                LastStage::Shebang => Detection::Shebang(lang),
                LastStage::Extension | LastStage::None => Detection::Extension(lang),
            })
        }
        _ => DetectResult::Ambiguous(candidates),
    })
}

#[derive(Copy, Clone)]
enum LastStage {
    None,
    Extension,
    Shebang,
    Heuristics,
}

fn filter_candidates(
    prev: Vec<&'static str>,
    new_: Vec<&'static str>,
) -> Vec<&'static str> {
    if prev.is_empty() {
        return new_;
    }
    if new_.is_empty() {
        return prev;
    }
    let filtered: Vec<_> = prev.iter().filter(|l| new_.contains(l)).copied().collect();
    if filtered.is_empty() {
        prev
    } else {
        filtered
    }
}

fn truncate_to_char_boundary(s: &str, mut max: usize) -> &str {
    if max >= s.len() {
        s
    } else {
        while !s.is_char_boundary(max) {
            max -= 1;
        }
        &s[..max]
    }
}
