use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::Path,
    process,
};

use clap::{App, Arg};
use hyperpolyglot::{
    detectors::{
        apply_specialization, classify, classify_tficf, get_extension, get_language_from_filename,
        get_languages_from_extension, get_languages_from_heuristics, get_languages_from_shebang,
    },
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
    chunked: bool,
}

impl Strategies {
    fn needs_file_read(&self) -> bool {
        self.shebang || self.heuristics || self.classifier
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
        .arg(Arg::with_name("chunked").long("chunked").help("Profile mode: for files > 150 KB, classify top / middle / bottom 50 KB chunks separately and print one verdict per chunk. Useful for files with mixed content or padding-decoy obfuscation. Smaller files keep the default single-verdict output."))
        .get_matches();

    let any = ["filename", "extension", "shebang", "heuristics", "classifier"]
        .iter()
        .any(|k| matches.is_present(k));
    let use_tficf = matches.is_present("tficf");
    let chunked = matches.is_present("chunked");
    let opts = if any {
        Strategies {
            filename: matches.is_present("filename"),
            extension: matches.is_present("extension"),
            shebang: matches.is_present("shebang"),
            heuristics: matches.is_present("heuristics"),
            classifier: matches.is_present("classifier"),
            use_tficf,
            chunked,
        }
    } else {
        Strategies {
            filename: true,
            extension: true,
            shebang: true,
            heuristics: true,
            classifier: true,
            use_tficf,
            chunked,
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

fn print_result(path: &Path, opts: &Strategies) {
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
    println!("{}: {}", path.display(), label);
}

/// 3-chunk profile output. Returns `Ok(true)` if chunked output was emitted,
/// `Ok(false)` if the file is below the threshold and the caller should fall
/// back to single-verdict output, or `Err` if I/O failed.
fn print_chunked_result(path: &Path, opts: &Strategies) -> io::Result<bool> {
    let size = std::fs::metadata(path)?.len() as usize;
    if size <= CHUNKED_THRESHOLD {
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
        let lang = if opts.use_tficf {
            classify_tficf(content, &candidates)
        } else {
            classify(content, &candidates)
        };
        let lang = apply_specialization(lang, content);
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
