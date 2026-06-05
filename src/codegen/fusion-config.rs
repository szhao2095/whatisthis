pub(crate) mod fusion_config {
    pub const W_STRUCTURE: f64 = 0.4;
    pub const W_BAYES: f64 = 0.15;
    pub const W_TFICF: f64 = 0.25;
    pub const W_LINEAR: f64 = 0.1;
    pub const W_ENTROPY_GATE: f64 = 0.1;
    pub const THRESHOLD_HIGH: f64 = 0.7;
    pub const THRESHOLD_MED: f64 = 0.4;
    pub const ABSTAIN_BELOW: f64 = 0.2;
    pub const AMBIGUITY_MARGIN: f64 = 0.05;
    pub const ENTROPY_SUPPRESS: f64 = 7.5;
    pub const ENTROPY_PRINTABLE_MIN: f64 = 0.2;
}
