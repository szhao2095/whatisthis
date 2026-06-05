use pcre2::bytes::Regex as PCRERegex;
use phf_codegen::Map as PhfMap;
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{BufWriter, Write},
    iter,
};

type LanguageMap = HashMap<String, LanguageDTO>;
type NamedPatterns = HashMap<String, MaybeMany<String>>;

#[derive(Deserialize)]
struct LanguageDTO {
    filenames: Option<Vec<String>>,
    interpreters: Option<Vec<String>>,
    extensions: Option<Vec<String>>,
    #[serde(rename(deserialize = "type"))]
    language_type: LanguageType,
    color: Option<String>,
    group: Option<String>,
}

impl LanguageDTO {
    fn to_domain_object_code(&self, name: &str) -> String {
        format!(
            "Language {{ name: \"{}\", language_type: {}, color: {:?}, group: {:?} }}",
            name,
            self.language_type.to_domain_object_code(),
            self.color,
            self.group
        )
    }
}

#[derive(Deserialize, Debug)]
enum LanguageType {
    #[serde(rename = "data")]
    Data,
    #[serde(rename = "markup")]
    Markup,
    #[serde(rename = "programming")]
    Programming,
    #[serde(rename = "prose")]
    Prose,
}

impl LanguageType {
    fn to_domain_object_code(&self) -> String {
        format!("LanguageType::{:?}", self)
    }
}

#[derive(Deserialize)]
struct Heuristics {
    disambiguations: Vec<Disambiguation>,
    named_patterns: NamedPatterns,
}

#[derive(Deserialize)]
struct Disambiguation {
    extensions: Vec<String>,
    rules: Vec<RuleDTO>,
}

impl Disambiguation {
    fn to_domain_object_code(&self, named_patterns: &NamedPatterns) -> String {
        let mut rules = String::new();
        for rule in self.rules.iter() {
            rules.push_str(format!("{},", rule.to_domain_object_code(named_patterns)).as_str());
        }
        format!("&[{}]", rules)
    }
}

#[derive(Deserialize)]
struct RuleDTO {
    language: MaybeMany<String>,
    #[serde(flatten)]
    pattern: Option<PatternDTO>,
}

impl RuleDTO {
    fn to_domain_object_code(&self, named_patterns: &NamedPatterns) -> String {
        let languages = match &self.language {
            MaybeMany::Many(values) => values.clone(),
            MaybeMany::One(value) => vec![value.clone()],
        };

        let pattern_code = match &self.pattern {
            Some(pattern) => format!("Some({})", pattern.to_domain_object_code(named_patterns)),
            None => String::from("None"),
        };

        format!(
            "Rule {{ languages: &[\"{}\"], pattern: {}}}",
            languages.join("\",\""),
            pattern_code
        )
    }
}

#[derive(Clone, Deserialize)]
enum PatternDTO {
    #[serde(rename = "and")]
    And(Vec<PatternDTO>),
    #[serde(rename = "named_pattern")]
    Named(String),
    #[serde(rename = "negative_pattern")]
    Negative(String),
    #[serde(rename = "pattern")]
    Positive(MaybeMany<String>),
}

impl PatternDTO {
    fn to_domain_object_code(&self, named_patterns: &NamedPatterns) -> String {
        match self {
            PatternDTO::Positive(MaybeMany::One(pattern)) => {
                // Panic on invalid regex now so we can unwrap in lib
                if let Err(e) = PCRERegex::new(pattern) {
                    panic!("Invalid regex pattern: {}\n{}", pattern, e);
                }
                format!("Pattern::Positive({:?})", pattern)
            }
            PatternDTO::Negative(pattern) => {
                // Panic on invalid regex now so we can unwrap in lib
                if let Err(e) = PCRERegex::new(pattern) {
                    panic!("Invalid regex pattern: {}\n{}", pattern, e);
                }
                format!("Pattern::Negative({:?})", pattern)
            }
            PatternDTO::Positive(MaybeMany::Many(patterns)) => {
                let mut code = String::from("Pattern::Or(&[");
                for pattern in patterns.iter() {
                    let p = PatternDTO::Positive(MaybeMany::One(pattern.clone()));
                    code.push_str(format!("{},", p.to_domain_object_code(named_patterns)).as_str());
                }
                code.push_str("])");
                code
            }
            PatternDTO::And(patterns) => {
                let mut code = String::from("Pattern::And(&[");
                for pattern in patterns.iter() {
                    code.push_str(
                        format!("{},", pattern.to_domain_object_code(named_patterns)).as_str(),
                    );
                }
                code.push_str("])");
                code
            }
            PatternDTO::Named(pattern_name) => {
                if let Some(pattern) = named_patterns.get(pattern_name) {
                    // Assume that all named patterns are positive
                    let pattern = PatternDTO::Positive(pattern.clone());
                    return pattern.to_domain_object_code(named_patterns);
                } else {
                    panic!(
                        "Named pattern: {} not found in named pattern map",
                        pattern_name
                    );
                };
            }
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum MaybeMany<T> {
    Many(Vec<T>),
    One(T),
}

const DISAMBIGUATION_HEURISTICS_FILE: &str = "src/codegen/disambiguation-heuristics-map.rs";
const EXTENSION_MAP_FILE: &str = "src/codegen/extension-language-map.rs";
const FILENAME_MAP_FILE: &str = "src/codegen/filename-language-map.rs";
const INTERPRETER_MAP_FILE: &str = "src/codegen/interpreter-language-map.rs";
const LANGUAGE_INFO_FILE: &str = "src/codegen/language-info-map.rs";
const LANGUAGE_LIST_FILE: &str = "src/codegen/languages.rs";
const TOKEN_LOG_PROBABILITY_FILE: &str = "src/codegen/token-log-probabilities.rs";
const TFICF_MODEL_FILE: &str = "src/codegen/tficf-model.rs";

const HEURISTICS_SOURCE_FILE: &str = "heuristics.yml";
const LANGUAGE_SOURCE_FILE: &str = "languages.yml";
const SPECIALIZATIONS_SOURCE_FILE: &str = "specializations.yml";
const SPECIALIZATIONS_OUTPUT_FILE: &str = "src/codegen/specializations-config.rs";
const MAGIC_SOURCE_FILE: &str = "magic.yml";
const MAGIC_OUTPUT_FILE: &str = "src/codegen/magic-config.rs";
const TAXONOMY_SOURCE_FILE: &str = "taxonomy.yml";
const TAXONOMY_OUTPUT_FILE: &str = "src/codegen/taxonomy-config.rs";
const CHARGRAM_MODEL_FILE: &str = "src/codegen/linear-chargram-model.rs";
const FUSION_SOURCE_FILE: &str = "fusion.yml";
const FUSION_OUTPUT_FILE: &str = "src/codegen/fusion-config.rs";

const MAX_TOKEN_BYTES: usize = 32;

fn main() {
    let languages: LanguageMap =
        serde_yaml::from_reader(File::open(LANGUAGE_SOURCE_FILE).unwrap()).unwrap();

    write_language_list(&languages);
    write_language_info(&languages);
    create_filename_map(&languages);
    create_interpreter_map(&languages);
    create_extension_map(&languages);

    let heuristics: Heuristics =
        serde_yaml::from_str(&fs::read_to_string(HEURISTICS_SOURCE_FILE).unwrap()[..]).unwrap();
    create_disambiguation_heuristics_map(heuristics);

    write_specializations_config(&languages);
    write_magic_config();
    write_taxonomy_config(&languages);
    write_fusion_config();

    train_classifier();
    train_tficf_classifier();
    train_chargram_classifier();
}

#[derive(Clone, Deserialize)]
struct SpecializationDTO {
    variant: String,
    #[serde(default)]
    base: Option<String>,
    pattern: String,
}

fn write_specializations_config(languages: &LanguageMap) {
    let rules: Vec<SpecializationDTO> = serde_yaml::from_str(
        &fs::read_to_string(SPECIALIZATIONS_SOURCE_FILE).unwrap()[..],
    )
    .unwrap();

    // Validate at codegen time: every referenced language exists, and every
    // pattern compiles as PCRE2. Catches typos before they reach runtime.
    for rule in &rules {
        assert!(
            languages.contains_key(&rule.variant[..]),
            "specializations.yml references variant {:?} which is not defined in languages.yml",
            rule.variant
        );
        if let Some(base) = &rule.base {
            assert!(
                languages.contains_key(&base[..]),
                "specializations.yml references base {:?} which is not defined in languages.yml",
                base
            );
        }
        pcre2::bytes::Regex::new(&rule.pattern).unwrap_or_else(|e| {
            panic!(
                "specializations.yml: pattern {:?} for variant {:?} does not compile: {}",
                rule.pattern, rule.variant, e
            )
        });
    }

    // Marker-authoritative rules (no `base`) fire first; base-conditional
    // rules act as a safety net for ambiguous markers.
    let mut sorted = rules;
    sorted.sort_by_key(|r| r.base.is_some());

    let mut file = BufWriter::new(File::create(SPECIALIZATIONS_OUTPUT_FILE).unwrap());
    writeln!(&mut file, "// @generated by codegen.rs from specializations.yml — do not hand-edit").unwrap();
    writeln!(&mut file).unwrap();
    writeln!(
        &mut file,
        "pub(crate) struct SpecializationRule {{\n    pub variant: &'static str,\n    pub base: Option<&'static str>,\n    pub pattern: &'static str,\n}}"
    )
    .unwrap();
    writeln!(&mut file).unwrap();
    writeln!(
        &mut file,
        "pub(crate) static SPECIALIZATIONS: &[SpecializationRule] = &["
    )
    .unwrap();
    for r in &sorted {
        let base_expr = match &r.base {
            Some(b) => format!("Some({:?})", b),
            None => "None".to_string(),
        };
        writeln!(
            &mut file,
            "    SpecializationRule {{ variant: {:?}, base: {}, pattern: {:?} }},",
            r.variant, base_expr, r.pattern,
        )
        .unwrap();
    }
    writeln!(&mut file, "];").unwrap();
}

#[derive(Deserialize)]
struct MagicRuleDTO {
    name: String,
    magic: String,
    offset: usize,
    suppress_text_classifier: bool,
}

fn write_magic_config() {
    let rules: Vec<MagicRuleDTO> = serde_yaml::from_str(
        &fs::read_to_string(MAGIC_SOURCE_FILE).unwrap_or_default()[..],
    )
    .expect("magic.yml parse error");

    let mut file = BufWriter::new(File::create(MAGIC_OUTPUT_FILE).unwrap());

    writeln!(&mut file, "pub(crate) struct MagicRule {{").unwrap();
    writeln!(&mut file, "    pub name: &'static str,").unwrap();
    writeln!(&mut file, "    pub magic_bytes: &'static [u8],").unwrap();
    writeln!(&mut file, "    pub offset: usize,").unwrap();
    writeln!(&mut file, "    pub suppress_text_classifier: bool,").unwrap();
    writeln!(&mut file, "}}").unwrap();
    writeln!(&mut file).unwrap();

    // Emit each rule's magic bytes as a standalone &[u8] static so the
    // outer static array can borrow them.
    for (i, rule) in rules.iter().enumerate() {
        let hex = rule.magic.trim();
        assert!(hex.len() % 2 == 0 && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "magic.yml: {:?} has invalid hex string {:?}", rule.name, hex);
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|j| u8::from_str_radix(&hex[j..j+2], 16).unwrap())
            .collect();
        let bytes_repr: Vec<String> = bytes.iter().map(|b| format!("0x{:02x}", b)).collect();
        writeln!(&mut file, "static MAGIC_BYTES_{}: &[u8] = &[{}];",
            i, bytes_repr.join(", ")).unwrap();
    }
    writeln!(&mut file).unwrap();

    writeln!(&mut file, "pub(crate) static MAGIC_RULES: &[MagicRule] = &[").unwrap();
    for (i, rule) in rules.iter().enumerate() {
        writeln!(
            &mut file,
            "    MagicRule {{ name: {:?}, magic_bytes: MAGIC_BYTES_{}, offset: {}, suppress_text_classifier: {} }},",
            rule.name, i, rule.offset, rule.suppress_text_classifier
        ).unwrap();
    }
    writeln!(&mut file, "];").unwrap();
}

#[derive(Deserialize)]
struct FusionWeights {
    structure: f64,
    bayes: f64,
    tficf: f64,
    linear: f64,
    entropy_gate: f64,
}

#[derive(Deserialize)]
struct FusionThresholds {
    high_confidence: f64,
    medium_confidence: f64,
    abstain_below: f64,
    ambiguity_margin: f64,
    entropy_suppress: f64,
    entropy_printable_min: f64,
}

#[derive(Deserialize)]
struct FusionConfig {
    weights: FusionWeights,
    thresholds: FusionThresholds,
}

fn write_fusion_config() {
    let cfg: FusionConfig = serde_yaml::from_str(
        &fs::read_to_string(FUSION_SOURCE_FILE).unwrap_or_default()[..],
    ).expect("fusion.yml parse error");

    let mut file = BufWriter::new(File::create(FUSION_OUTPUT_FILE).unwrap());
    writeln!(&mut file, "pub(crate) mod fusion_config {{").unwrap();
    writeln!(&mut file, "    pub const W_STRUCTURE: f64 = {:?};", cfg.weights.structure).unwrap();
    writeln!(&mut file, "    pub const W_BAYES: f64 = {:?};", cfg.weights.bayes).unwrap();
    writeln!(&mut file, "    pub const W_TFICF: f64 = {:?};", cfg.weights.tficf).unwrap();
    writeln!(&mut file, "    pub const W_LINEAR: f64 = {:?};", cfg.weights.linear).unwrap();
    writeln!(&mut file, "    pub const W_ENTROPY_GATE: f64 = {:?};", cfg.weights.entropy_gate).unwrap();
    writeln!(&mut file, "    pub const THRESHOLD_HIGH: f64 = {:?};", cfg.thresholds.high_confidence).unwrap();
    writeln!(&mut file, "    pub const THRESHOLD_MED: f64 = {:?};", cfg.thresholds.medium_confidence).unwrap();
    writeln!(&mut file, "    pub const ABSTAIN_BELOW: f64 = {:?};", cfg.thresholds.abstain_below).unwrap();
    writeln!(&mut file, "    pub const AMBIGUITY_MARGIN: f64 = {:?};", cfg.thresholds.ambiguity_margin).unwrap();
    writeln!(&mut file, "    pub const ENTROPY_SUPPRESS: f64 = {:?};", cfg.thresholds.entropy_suppress).unwrap();
    writeln!(&mut file, "    pub const ENTROPY_PRINTABLE_MIN: f64 = {:?};", cfg.thresholds.entropy_printable_min).unwrap();
    writeln!(&mut file, "}}").unwrap();
}

fn write_taxonomy_config(languages: &LanguageMap) {
    // taxonomy.yml: "LanguageName: FamilyName"
    let raw = fs::read_to_string(TAXONOMY_SOURCE_FILE).unwrap_or_default();
    let mapping: HashMap<String, String> = serde_yaml::from_str(&raw)
        .expect("taxonomy.yml parse error");

    // Warn (but don't panic) for names not in languages.yml — taxonomy may
    // intentionally include variants that aren't in the base corpus.
    for lang in mapping.keys() {
        if !languages.contains_key(lang.as_str()) {
            eprintln!("taxonomy.yml: {:?} not in languages.yml (skipping)", lang);
        }
    }

    let mut map = PhfMap::new();
    for (lang, family) in &mapping {
        map.entry(lang.as_str(), &format!("{:?}", family));
    }

    let mut file = BufWriter::new(File::create(TAXONOMY_OUTPUT_FILE).unwrap());
    writeln!(
        &mut file,
        "static TAXONOMY: phf::Map<&'static str, &'static str> =\n{};\n",
        map.build()
    )
    .unwrap();
}

fn write_language_list(languages: &LanguageMap) {
    let mut file = BufWriter::new(File::create(LANGUAGE_LIST_FILE).unwrap());

    let languages: Vec<String> = languages.keys().map(|language| language.clone()).collect();

    writeln!(
        &mut file,
        "static LANGUAGES: &[&'static str] = &[\"{}\"];",
        languages.join("\",\"")
    )
    .unwrap();
}

fn write_language_info(languages: &LanguageMap) {
    let mut file = BufWriter::new(File::create(LANGUAGE_INFO_FILE).unwrap());

    let mut language_info_map = PhfMap::new();
    for (language_name, language) in languages.iter() {
        language_info_map.entry(
            &language_name[..],
            &language.to_domain_object_code(&language_name[..])[..],
        );
    }

    writeln!(
        &mut file,
        "static LANGUAGE_INFO: phf::Map<&'static str, Language> =\n{};\n",
        language_info_map.build()
    )
    .unwrap();
}

fn create_filename_map(languages: &LanguageMap) {
    let mut file = BufWriter::new(File::create(FILENAME_MAP_FILE).unwrap());

    let mut filename_to_language_map = PhfMap::new();
    for (language_name, language) in languages.iter() {
        if let Some(filenames) = &language.filenames {
            for filename in filenames.iter() {
                filename_to_language_map
                    .entry(&filename[..], &format!("\"{}\"", language_name)[..]);
            }
        }
    }

    writeln!(
        &mut file,
        "static FILENAMES: phf::Map<&'static str, &'static str> =\n{};\n",
        filename_to_language_map.build()
    )
    .unwrap();
}

fn create_interpreter_map(languages: &LanguageMap) {
    let mut file = BufWriter::new(File::create(INTERPRETER_MAP_FILE).unwrap());

    let mut temp_map: HashMap<&String, Vec<&String>> = HashMap::new();
    for (language_name, language) in languages.iter() {
        if let Some(interpreters) = &language.interpreters {
            for interpreter in interpreters.iter() {
                match temp_map.get_mut(interpreter) {
                    Some(entry) => {
                        entry.push(language_name);
                    }
                    None => {
                        temp_map.insert(interpreter, vec![language_name]);
                    }
                }
            }
        }
    }

    let mut interpreter_to_language_map = PhfMap::new();
    for (interpreter, languages) in temp_map.iter() {
        interpreter_to_language_map.entry(&interpreter[..], &format!("&{:?}", languages)[..]);
    }

    writeln!(
        &mut file,
        "static INTERPRETERS: phf::Map<&'static str, &[&str]> =\n{};\n",
        interpreter_to_language_map.build()
    )
    .unwrap();
}

fn create_extension_map(languages: &LanguageMap) {
    let mut file = BufWriter::new(File::create(EXTENSION_MAP_FILE).unwrap());

    let mut temp_map: HashMap<String, Vec<&String>> = HashMap::new();
    for (language_name, language) in languages.iter() {
        if let Some(extensions) = &language.extensions {
            for extension in extensions.iter() {
                let extension = extension.clone().to_ascii_lowercase();
                match temp_map.get_mut(&extension) {
                    Some(entry) => {
                        entry.push(language_name);
                    }
                    None => {
                        temp_map.insert(extension.clone(), vec![language_name]);
                    }
                }
            }
        }
    }

    let mut extension_to_language_map = PhfMap::new();
    for (extension, languages) in temp_map.iter() {
        extension_to_language_map.entry(&extension[..], &format!("&{:?}", languages)[..]);
    }

    writeln!(
        &mut file,
        "static EXTENSIONS: phf::Map<&'static str, &[&str]> =\n{};\n",
        extension_to_language_map.build()
    )
    .unwrap();
}

fn create_disambiguation_heuristics_map(heuristics: Heuristics) {
    let mut file = BufWriter::new(File::create(DISAMBIGUATION_HEURISTICS_FILE).unwrap());

    let mut temp_map: HashMap<String, String> = HashMap::new();
    for mut dis in heuristics.disambiguations.into_iter() {
        for ext in dis.extensions.iter() {
            // Adding a rule to default to C for .h if the Objective C and C++ patterns don't match
            // The classifer was unreliable for distinguishing between C and C++ for .h
            if ext == ".h" {
                dis.rules.push(RuleDTO {
                    language: MaybeMany::One(String::from("C")),
                    pattern: None,
                });
            }
            let extension = ext.clone().to_ascii_lowercase();
            let key = extension;
            let value = dis.to_domain_object_code(&heuristics.named_patterns);
            temp_map.insert(key, value);
        }
    }

    let mut disambiguation_heuristic_map = PhfMap::new();
    for (key, value) in temp_map.iter() {
        disambiguation_heuristic_map.entry(&key[..], &value[..]);
    }

    writeln!(
        &mut file,
        "static DISAMBIGUATIONS: phf::Map<&'static str, &'static [Rule]> =\n{};\n",
        disambiguation_heuristic_map.build()
    )
    .unwrap();
}

fn train_classifier() {
    let mut temp_token_count: HashMap<String, HashMap<String, i32>> = HashMap::new();
    let mut temp_total_tokens_count = HashMap::new();

    fs::read_dir("samples")
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.path().is_dir())
        .map(|language_dir| {
            let path = language_dir.path();
            let language = path.file_name().unwrap();
            let language = language.to_string_lossy().into_owned();
            let language = match &language[..] {
                "Fstar" => String::from("F*"),
                _ => language,
            };

            let file_paths = fs::read_dir(language_dir.path())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.is_file());

            let language_iter = iter::repeat(language);
            file_paths.zip(language_iter)
        })
        .flatten()
        .for_each(|(entry, language)| {
            let content = fs::read(entry).unwrap();

            // When tokenizing an invalid utf8 string, just set it to ""
            // Add better error handling here in the future but unure of the best
            // way to handle it now
            let tokens =
                polyglot_tokenizer::get_key_tokens(std::str::from_utf8(&content[..]).unwrap_or(""));

            for token in tokens {
                if token.len() <= MAX_TOKEN_BYTES {
                    let total_tokens = temp_total_tokens_count.entry(language.clone()).or_insert(0);
                    *total_tokens += 1;

                    let tokens_count = temp_token_count
                        .entry(language.clone())
                        .or_insert(HashMap::new());

                    let count = tokens_count.entry(String::from(token)).or_insert(0);
                    *count += 1;
                }
            }
        });

    // Write token log probabilities
    let mut file = BufWriter::new(File::create(TOKEN_LOG_PROBABILITY_FILE).unwrap());
    let mut language_token_log_probabilities = PhfMap::new();
    for (language, token_count_map) in temp_token_count.iter() {
        let total_tokens = *temp_total_tokens_count.get(language).unwrap() as f64;
        let mut token_log_probabilities = PhfMap::new();
        for (token, token_count) in token_count_map.iter() {
            let probability = (*token_count as f64) / (total_tokens);
            let log_probability = probability.ln();
            token_log_probabilities.entry(&token[..], &format!("{}f64", log_probability)[..]);
        }
        let codegen_log_prob_map = format!("{}", token_log_probabilities.build());
        language_token_log_probabilities.entry(&language[..], &codegen_log_prob_map[..]);
    }

    writeln!(
        &mut file,
        "static TOKEN_LOG_PROBABILITIES: phf::Map<&'static str, phf::Map<&'static str, f64>> =\n{};\n",
        language_token_log_probabilities.build()
    )
    .unwrap();
}

const TFICF_MIN_DOCUMENT_FREQUENCY: u32 = 2;
// Must match TF_CAP in src/detectors/tficf_classifier.rs.
const TFICF_TF_CAP: u32 = 100;

fn train_tficf_classifier() {
    let mut samples_by_lang: HashMap<String, Vec<HashMap<String, u32>>> = HashMap::new();

    for entry in fs::read_dir("samples").unwrap() {
        let dir = entry.unwrap();
        if !dir.path().is_dir() {
            continue;
        }
        let raw_name = dir.file_name().to_string_lossy().into_owned();
        let lang = match &raw_name[..] {
            "Fstar" => String::from("F*"),
            _ => raw_name,
        };

        for file_entry in fs::read_dir(dir.path()).unwrap() {
            let file_entry = file_entry.unwrap();
            if !file_entry.path().is_file() {
                continue;
            }
            let content = fs::read(file_entry.path()).unwrap();
            let content_str = std::str::from_utf8(&content[..]).unwrap_or("");

            let mut tf: HashMap<String, u32> = HashMap::new();
            for token in polyglot_tokenizer::get_linguist_tokens(content_str) {
                if token.len() <= MAX_TOKEN_BYTES {
                    *tf.entry(token.into_owned()).or_insert(0) += 1;
                }
            }
            if !tf.is_empty() {
                samples_by_lang
                    .entry(lang.clone())
                    .or_insert_with(Vec::new)
                    .push(tf);
            }
        }
    }

    let mut docfreq: HashMap<String, u32> = HashMap::new();
    for samples in samples_by_lang.values() {
        for sample in samples {
            for token in sample.keys() {
                *docfreq.entry(token.clone()).or_insert(0) += 1;
            }
        }
    }

    let mut vocab: Vec<String> = docfreq
        .iter()
        .filter_map(|(t, &c)| {
            if c >= TFICF_MIN_DOCUMENT_FREQUENCY {
                Some(t.clone())
            } else {
                None
            }
        })
        .collect();
    vocab.sort();
    let term_to_idx: HashMap<String, u32> = vocab
        .iter()
        .enumerate()
        .map(|(i, t)| (t.clone(), i as u32))
        .collect();

    let num_langs = samples_by_lang.len() as f64;
    let mut icf: Vec<f64> = vec![0.0; vocab.len()];
    for samples in samples_by_lang.values() {
        let mut terms_in_lang: HashSet<u32> = HashSet::new();
        for sample in samples {
            for token in sample.keys() {
                if let Some(&idx) = term_to_idx.get(token) {
                    terms_in_lang.insert(idx);
                }
            }
        }
        for idx in terms_in_lang {
            icf[idx as usize] += 1.0;
        }
    }
    for v in icf.iter_mut() {
        *v = (num_langs / *v).ln() + 1.0;
    }

    let mut centroids: HashMap<String, Vec<(u32, f64)>> = HashMap::new();
    for (lang, samples) in samples_by_lang.iter() {
        let mut centroid: HashMap<u32, f64> = HashMap::new();
        let n = samples.len() as f64;
        for sample in samples {
            let mut svec: HashMap<u32, f64> = HashMap::new();
            for (token, &freq) in sample {
                if let Some(&idx) = term_to_idx.get(token) {
                    let capped = freq.min(TFICF_TF_CAP);
                    let tf = 1.0 + (capped as f64).ln();
                    svec.insert(idx, tf * icf[idx as usize]);
                }
            }
            let norm = svec.values().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 0.0 {
                for v in svec.values_mut() {
                    *v /= norm;
                }
            }
            for (idx, v) in svec {
                *centroid.entry(idx).or_insert(0.0) += v;
            }
        }
        for v in centroid.values_mut() {
            *v /= n;
        }
        let norm = centroid.values().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 0.0 {
            for v in centroid.values_mut() {
                *v /= norm;
            }
        }
        let mut sorted: Vec<(u32, f64)> = centroid.into_iter().collect();
        sorted.sort_by_key(|x| x.0);
        centroids.insert(lang.clone(), sorted);
    }

    let mut file = BufWriter::new(File::create(TFICF_MODEL_FILE).unwrap());

    let mut idx_value_strs: HashMap<String, String> = HashMap::new();
    for (token, &idx) in term_to_idx.iter() {
        idx_value_strs.insert(token.clone(), format!("{}u32", idx));
    }
    let mut vocab_map = PhfMap::new();
    for (token, value) in idx_value_strs.iter() {
        vocab_map.entry(&token[..], &value[..]);
    }
    writeln!(
        &mut file,
        "static TFICF_VOCABULARY: phf::Map<&'static str, u32> =\n{};\n",
        vocab_map.build()
    )
    .unwrap();

    write!(&mut file, "static TFICF_ICF: &[f64] = &[").unwrap();
    for v in &icf {
        write!(&mut file, "{:?}f64,", v).unwrap();
    }
    writeln!(&mut file, "];\n").unwrap();

    let mut centroid_value_strs: HashMap<String, String> = HashMap::new();
    for (lang, sparse) in centroids.iter() {
        let mut s = String::from("&[");
        for (idx, val) in sparse.iter() {
            s.push_str(&format!("({}u32,{:?}f64),", idx, val));
        }
        s.push(']');
        centroid_value_strs.insert(lang.clone(), s);
    }
    let mut centroid_map = PhfMap::new();
    for (lang, value) in centroid_value_strs.iter() {
        centroid_map.entry(&lang[..], &value[..]);
    }
    writeln!(
        &mut file,
        "static TFICF_CENTROIDS: phf::Map<&'static str, &'static [(u32, f64)]> =\n{};\n",
        centroid_map.build()
    )
    .unwrap();
}

// ── Char n-gram linear classifier ────────────────────────────────────────────

/// Minimum number of files a char n-gram must appear in to enter the vocab.
/// Higher than TF-ICF's 2 because the raw char n-gram space is enormous.
const CHARGRAM_MIN_DOC_FREQ: u32 = 8;

/// Maximum vocabulary size (most discriminative n-grams by ICF).
/// Keeps the generated model file to roughly the same size as tficf-model.rs.
const CHARGRAM_MAX_VOCAB: usize = 60_000;

/// n-gram sizes to extract. 4-grams are the sweet spot: distinctive without
/// the vocabulary explosion of 3-grams or sparsity of 5-grams.
const CHARGRAM_NS: &[usize] = &[4];

/// Maximum bytes read from each sample file during training. Caps training
/// time for very large sample files without sacrificing much accuracy.
const CHARGRAM_TRAIN_BYTES: usize = 4_000;

/// Same TF cap as TF-ICF so the math is comparable.
const CHARGRAM_TF_CAP: u32 = 100;

fn extract_ngrams(bytes: &[u8], n: usize) -> impl Iterator<Item = &[u8]> {
    bytes.windows(n).filter(|w| !w.contains(&b'\n') && !w.contains(&b'\r'))
}

fn train_chargram_classifier() {
    let mut samples_by_lang: HashMap<String, Vec<HashMap<Vec<u8>, u32>>> = HashMap::new();

    for entry in fs::read_dir("samples").unwrap() {
        let dir = entry.unwrap();
        if !dir.path().is_dir() { continue; }
        let raw_name = dir.file_name().to_string_lossy().into_owned();
        let lang = match &raw_name[..] {
            "Fstar" => String::from("F*"),
            _ => raw_name,
        };
        for file_entry in fs::read_dir(dir.path()).unwrap() {
            let file_entry = file_entry.unwrap();
            if !file_entry.path().is_file() { continue; }
            let full = fs::read(file_entry.path()).unwrap();
            let bytes = &full[..full.len().min(CHARGRAM_TRAIN_BYTES)];

            let mut tf: HashMap<Vec<u8>, u32> = HashMap::new();
            for &n in CHARGRAM_NS {
                for gram in extract_ngrams(bytes, n) {
                    *tf.entry(gram.to_vec()).or_insert(0) += 1;
                }
            }
            if !tf.is_empty() {
                samples_by_lang.entry(lang.clone()).or_insert_with(Vec::new).push(tf);
            }
        }
    }

    // Document frequency (how many files contain each gram).
    let mut docfreq: HashMap<Vec<u8>, u32> = HashMap::new();
    for samples in samples_by_lang.values() {
        for sample in samples {
            for gram in sample.keys() {
                *docfreq.entry(gram.clone()).or_insert(0) += 1;
            }
        }
    }

    // Vocabulary: grams with enough document frequency and valid UTF-8.
    // To keep the model file size manageable, cap at CHARGRAM_MAX_VOCAB entries
    // keeping the ones with the highest raw document frequency (most common =
    // most likely to have discriminative weight across many languages).
    let mut vocab_pairs: Vec<(String, u32)> = docfreq
        .iter()
        .filter(|(g, &c)| c >= CHARGRAM_MIN_DOC_FREQ && std::str::from_utf8(g).is_ok())
        .map(|(g, &c)| (std::str::from_utf8(g).unwrap().to_string(), c))
        .collect();
    // Sort descending by doc-freq, then alphabetically for stability.
    vocab_pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    vocab_pairs.truncate(CHARGRAM_MAX_VOCAB);
    let mut vocab: Vec<String> = vocab_pairs.into_iter().map(|(s, _)| s).collect();
    vocab.sort(); // sort lexicographically for the PHF map
    let term_to_idx: HashMap<String, u32> = vocab.iter().enumerate().map(|(i, t)| (t.clone(), i as u32)).collect();

    let num_langs = samples_by_lang.len() as f64;
    let mut icf: Vec<f64> = vec![0.0; vocab.len()];
    for samples in samples_by_lang.values() {
        let mut terms_in_lang: HashSet<u32> = HashSet::new();
        for sample in samples {
            for gram in sample.keys() {
                if let Ok(s) = std::str::from_utf8(gram) {
                    if let Some(&idx) = term_to_idx.get(s) {
                        terms_in_lang.insert(idx);
                    }
                }
            }
        }
        for idx in terms_in_lang {
            icf[idx as usize] += 1.0;
        }
    }
    for v in icf.iter_mut() {
        *v = (num_langs / v.max(1.0)).ln() + 1.0;
    }

    // Compute per-language centroids (same as TF-ICF).
    let mut centroids: HashMap<String, Vec<(u32, f64)>> = HashMap::new();
    for (lang, samples) in samples_by_lang.iter() {
        let mut centroid: HashMap<u32, f64> = HashMap::new();
        let n = samples.len() as f64;
        for sample in samples {
            let mut svec: HashMap<u32, f64> = HashMap::new();
            for (gram, &freq) in sample {
                if let Ok(s) = std::str::from_utf8(gram) {
                    if let Some(&idx) = term_to_idx.get(s) {
                        let capped = freq.min(CHARGRAM_TF_CAP);
                        let tf = 1.0 + (capped as f64).ln();
                        svec.insert(idx, tf * icf[idx as usize]);
                    }
                }
            }
            let norm = svec.values().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 0.0 { for v in svec.values_mut() { *v /= norm; } }
            for (idx, v) in svec { *centroid.entry(idx).or_insert(0.0) += v; }
        }
        for v in centroid.values_mut() { *v /= n; }
        let norm = centroid.values().map(|x| x * x).sum::<f64>().sqrt();
        if norm > 0.0 { for v in centroid.values_mut() { *v /= norm; } }
        let mut sorted: Vec<(u32, f64)> = centroid.into_iter().collect();
        sorted.sort_by_key(|x| x.0);
        centroids.insert(lang.clone(), sorted);
    }

    let mut file = BufWriter::new(File::create(CHARGRAM_MODEL_FILE).unwrap());

    let mut idx_strs: HashMap<String, String> = HashMap::new();
    for (token, &idx) in term_to_idx.iter() {
        idx_strs.insert(token.clone(), format!("{}u32", idx));
    }
    let mut vocab_map = PhfMap::new();
    for (token, value) in idx_strs.iter() {
        vocab_map.entry(&token[..], &value[..]);
    }
    writeln!(&mut file, "static CHARGRAM_VOCABULARY: phf::Map<&'static str, u32> =\n{};\n", vocab_map.build()).unwrap();

    write!(&mut file, "static CHARGRAM_ICF: &[f64] = &[").unwrap();
    for v in &icf { write!(&mut file, "{:?}f64,", v).unwrap(); }
    writeln!(&mut file, "];\n").unwrap();

    let mut centroid_strs: HashMap<String, String> = HashMap::new();
    for (lang, sparse) in centroids.iter() {
        let mut s = String::from("&[");
        for (idx, val) in sparse.iter() { s.push_str(&format!("({}u32,{:?}f64),", idx, val)); }
        s.push(']');
        centroid_strs.insert(lang.clone(), s);
    }
    let mut centroid_map = PhfMap::new();
    for (lang, value) in centroid_strs.iter() { centroid_map.entry(&lang[..], &value[..]); }
    writeln!(&mut file, "static CHARGRAM_CENTROIDS: phf::Map<&'static str, &'static [(u32, f64)]> =\n{};\n", centroid_map.build()).unwrap();
}
