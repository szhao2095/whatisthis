mod classifier;
mod extensions;
mod filenames;
mod heuristics;
mod interpreters;
mod specializations;
mod tficf_classifier;

pub use classifier::classify;
pub use extensions::{get_extension, get_languages_from_extension};
pub use filenames::get_language_from_filename;
pub use heuristics::get_languages_from_heuristics;
pub use interpreters::get_languages_from_shebang;
pub use specializations::apply_specialization;
pub use tficf_classifier::classify_tficf;
