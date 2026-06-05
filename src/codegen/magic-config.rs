pub(crate) struct MagicRule {
    pub name: &'static str,
    pub magic_bytes: &'static [u8],
    pub offset: usize,
    pub suppress_text_classifier: bool,
}

static MAGIC_BYTES_0: &[u8] = &[0x7f, 0x45, 0x4c, 0x46];
static MAGIC_BYTES_1: &[u8] = &[0x4d, 0x5a];
static MAGIC_BYTES_2: &[u8] = &[0x25, 0x50, 0x44, 0x46];
static MAGIC_BYTES_3: &[u8] = &[0x50, 0x4b, 0x03, 0x04];
static MAGIC_BYTES_4: &[u8] = &[0x1f, 0x8b];
static MAGIC_BYTES_5: &[u8] = &[0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
static MAGIC_BYTES_6: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
static MAGIC_BYTES_7: &[u8] = &[0x53, 0x51, 0x4c, 0x69, 0x74, 0x65, 0x20, 0x66, 0x6f, 0x72, 0x6d, 0x61, 0x74, 0x20, 0x33, 0x00];
static MAGIC_BYTES_8: &[u8] = &[0xff, 0xd8, 0xff];
static MAGIC_BYTES_9: &[u8] = &[0x00, 0x61, 0x73, 0x6d];

pub(crate) static MAGIC_RULES: &[MagicRule] = &[
    MagicRule { name: "ELF", magic_bytes: MAGIC_BYTES_0, offset: 0, suppress_text_classifier: true },
    MagicRule { name: "PE", magic_bytes: MAGIC_BYTES_1, offset: 0, suppress_text_classifier: true },
    MagicRule { name: "PDF", magic_bytes: MAGIC_BYTES_2, offset: 0, suppress_text_classifier: false },
    MagicRule { name: "ZIP", magic_bytes: MAGIC_BYTES_3, offset: 0, suppress_text_classifier: false },
    MagicRule { name: "GZIP", magic_bytes: MAGIC_BYTES_4, offset: 0, suppress_text_classifier: true },
    MagicRule { name: "OLE2", magic_bytes: MAGIC_BYTES_5, offset: 0, suppress_text_classifier: false },
    MagicRule { name: "PNG", magic_bytes: MAGIC_BYTES_6, offset: 0, suppress_text_classifier: true },
    MagicRule { name: "SQLite", magic_bytes: MAGIC_BYTES_7, offset: 0, suppress_text_classifier: true },
    MagicRule { name: "JPEG", magic_bytes: MAGIC_BYTES_8, offset: 0, suppress_text_classifier: true },
    MagicRule { name: "WebAssembly", magic_bytes: MAGIC_BYTES_9, offset: 0, suppress_text_classifier: true },
];
