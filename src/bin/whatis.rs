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

#[derive(Copy, Clone)]
struct Strategies {
    filename: bool,
    extension: bool,
    shebang: bool,
    heuristics: bool,
    classifier: bool,
    use_tficf: bool,
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
        .get_matches();

    let any = ["filename", "extension", "shebang", "heuristics", "classifier"]
        .iter()
        .any(|k| matches.is_present(k));
    let use_tficf = matches.is_present("tficf");
    let opts = if any {
        Strategies {
            filename: matches.is_present("filename"),
            extension: matches.is_present("extension"),
            shebang: matches.is_present("shebang"),
            heuristics: matches.is_present("heuristics"),
            classifier: matches.is_present("classifier"),
            use_tficf,
        }
    } else {
        Strategies {
            filename: true,
            extension: true,
            shebang: true,
            heuristics: true,
            classifier: true,
            use_tficf,
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
    let label = match detect_with(path, opts) {
        Ok(DetectResult::Found(d)) => format!("{} [{}]", d.language(), d.variant()),
        Ok(DetectResult::Ambiguous(cands)) => format!("<ambiguous: {}>", cands.join(", ")),
        Ok(DetectResult::Unknown) => "<unknown>".to_string(),
        Err(e) => format!("<error: {}>", e),
    };
    println!("{}: {}", path.display(), label);
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
