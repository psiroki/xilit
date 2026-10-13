//! Diagnostic codes for every error and warning xilit can produce.
//!
//! Codes look like `E0302` / `W0508`: a severity letter followed by a
//! 4-digit number. The *number* is unique across errors and warnings, so
//! changing your mind about severity means changing only the first letter
//! (here, in [`codes`]) — plus, of course, the call site's control flow.
//!
//! Numbers are grouped by area: 00xx general, 01xx command line/shell,
//! 02xx quoting/references, 03xx model loading, 04xx accessors, 05xx vertex
//! format, 06xx mesh export. Keep `DIAGNOSTICS.md` in sync (a test checks).
//!
//! Usage:
//! * `bail_code!(codes::X, "fmt {}", args)` — return an error early;
//! * `err_code!(codes::X, "fmt {}", args)` — build an error value;
//! * `warn_code!(codes::W, "fmt {}", args)` — print a non-fatal warning.
//!
//! Errors travel as `anyhow::Error` with a [`Diagnostic`] as the root cause;
//! `.context(..)` layers added on top are kept in the message, and
//! [`report_error`] finds the code underneath them.

use std::fmt;

/// A diagnostic code such as `E0302`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code(&'static str);

impl Code {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn as_str(self) -> &'static str {
        self.0
    }
    pub fn is_error(self) -> bool {
        self.0.starts_with('E')
    }
    pub fn is_warning(self) -> bool {
        self.0.starts_with('W')
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// The root cause of every xilit error: a code plus a message.
#[derive(Debug)]
pub struct Diagnostic {
    pub code: Code,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Diagnostic {}

pub fn error(code: Code, message: String) -> anyhow::Error {
    debug_assert!(code.is_error(), "{code} is not an error code");
    anyhow::Error::new(Diagnostic { code, message })
}

pub fn warn(code: Code, message: fmt::Arguments<'_>) {
    debug_assert!(code.is_warning(), "{code} is not a warning code");
    eprintln!("warning[{code}]: {message}");
}

/// The code of the first [`Diagnostic`] in `e`'s chain, or
/// [`codes::UNCLASSIFIED`] if the failure was never given a code.
pub fn code_of(e: &anyhow::Error) -> Code {
    e.chain()
        .find_map(|cause| cause.downcast_ref::<Diagnostic>())
        .map_or(codes::UNCLASSIFIED, |d| d.code)
}

pub fn report_error(e: &anyhow::Error) {
    eprintln!("error[{}]: {e:#}", code_of(e));
}

#[allow(unused_macros)]
macro_rules! err_code {
    ($code:expr, $($arg:tt)+) => {
        $crate::diag::error($code, format!($($arg)+))
    };
}

#[allow(unused_macros)]
macro_rules! bail_code {
    ($code:expr, $($arg:tt)+) => {
        return Err($crate::diag::error($code, format!($($arg)+)))
    };
}

#[allow(unused_macros)]
macro_rules! warn_code {
    ($code:expr, $($arg:tt)+) => {
        $crate::diag::warn($code, format_args!($($arg)+))
    };
}

macro_rules! registry {
    ($($name:ident = $code:literal, $summary:literal;)*) => {
        pub mod codes {
            use super::Code;
            $(pub const $name: Code = Code($code);)*
        }

        /// Every code with a one-line summary (mirrored in DIAGNOSTICS.md).
        #[cfg_attr(not(test), allow(dead_code))]
        pub const ALL: &[(Code, &str)] = &[$((codes::$name, $summary)),*];
    };
}

registry! {
    // 00xx general
    UNCLASSIFIED = "E0001", "Unclassified failure (an internal error path is missing a code)";

    // 01xx command line / shell
    CONFLICTING_MODES = "E0101", "`-c` and `-s` were both given";
    UNKNOWN_COMMAND = "E0102", "Unknown command";
    BAD_USAGE = "E0103", "Wrong or missing subcommand/arguments for a command";
    UNEXPECTED_ARGUMENT = "E0104", "Unexpected extra argument";
    MISSING_OPTION_VALUE = "E0105", "An option that needs a value was given none";
    SCRIPT_READ_FAILED = "E0106", "Could not read the script file";
    STDIN_READ_FAILED = "E0107", "Could not read from stdin in the interactive shell";

    // 02xx quoting / object references
    UNTERMINATED_QUOTE = "E0201", "Unterminated quoted string";
    INVALID_ESCAPE = "E0202", "Invalid escape sequence in a quoted string";
    INVALID_OBJECT_REFERENCE = "E0203", "Malformed object reference (expected `\"Name\"` or `#index`)";
    INVALID_PRIMITIVE_REFERENCE = "E0204", "Malformed `<mesh>/<primitiveIndex>` reference";

    // 03xx model loading
    MODEL_READ_FAILED = "E0301", "Could not read the model file";
    GLTF_INVALID = "E0302", "The glTF failed validation (`--lenient` may load it anyway)";
    GLTF_UNPARSEABLE = "E0303", "The glTF could not be parsed even without validation";
    GLB_CHUNK_MISSING = "E0304", "A buffer refers to the GLB binary chunk, but there is none";
    DATA_URI_MALFORMED = "E0305", "Malformed `data:` URI in a buffer";
    DATA_URI_UNSUPPORTED = "E0306", "Unsupported `data:` URI (only base64 payloads are supported)";
    DATA_URI_DECODE_FAILED = "E0307", "Could not base64-decode a `data:` URI";
    BUFFER_FILE_READ_FAILED = "E0308", "Could not read an external buffer file";
    BUFFER_TOO_SHORT = "E0309", "A buffer holds less data than it declares";
    VALIDATION_IGNORED = "W0310", "glTF validation problems were ignored because of `--lenient`";

    // 04xx accessor reading
    ACCESSOR_SPARSE = "E0401", "Sparse accessors are not supported";
    ACCESSOR_NO_VIEW = "E0402", "Accessor has no buffer view";
    ACCESSOR_BAD_BUFFER = "E0403", "Accessor refers to a nonexistent buffer";
    ACCESSOR_OUT_OF_BOUNDS = "E0404", "Accessor extends past the end of its buffer";
    SOURCE_NOT_FLOAT = "E0405", "A source attribute is not floating-point (no conversion is done)";
    INDEX_BAD_TYPE = "E0406", "Index accessor is not UNSIGNED_BYTE/SHORT/INT";

    // 05xx vertex format
    VFMT_EMPTY_FIELD = "E0501", "Empty field in the vertex format";
    VFMT_EMPTY_NAME = "E0502", "Empty attribute name in the vertex format";
    VFMT_UNKNOWN_BINDING = "E0503", "Unknown binding in the vertex format";
    VFMT_BAD_PADDING = "E0504", "Padding amount is zero or too large";
    VFMT_NAMED_PADDING = "E0505", "`padN` must use the hidden name `_`";
    VFMT_DUPLICATE_SOURCE = "E0506", "A source binding is used more than once";
    VFMT_TOO_LARGE = "E0507", "The vertex layout is larger than 4 GiB";
    VFMT_BAD_GLSL_NAME = "W0508", "An emitted attribute name is not a valid GLSL identifier";

    // 06xx mesh export
    MESH_NOT_FOUND = "E0601", "No such mesh";
    PRIMITIVE_NOT_FOUND = "E0602", "The mesh has no such primitive";
    PRIMITIVE_NOT_TRIANGLES = "E0603", "Primitive mode is not TRIANGLES";
    PRIMITIVE_NOT_INDEXED = "E0604", "Primitive has no index accessor";
    INDEX_OVERFLOW = "E0605", "An index does not fit into u16";
    MISSING_POSITION = "E0606", "Primitive has no POSITION attribute";
    SOURCE_MISSING = "E0607", "A requested source attribute does not exist in the primitive";
    SOURCE_COUNT_MISMATCH = "E0608", "A source attribute's vertex count differs from POSITION";
    SOURCE_COMPONENTS_MISMATCH = "E0609", "A source attribute has an unexpected component count";
    OUTPUT_CREATE_FAILED = "E0610", "Could not create the output file";
    OUTPUT_WRITE_FAILED = "E0611", "Could not write the output file";
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_are_well_formed_and_numbers_unique() {
        let mut numbers = HashSet::new();
        for (code, summary) in ALL {
            let s = code.as_str();
            assert!(s.len() == 5, "{s}: expected a letter and 4 digits");
            assert!(code.is_error() || code.is_warning(), "{s}: bad severity letter");
            assert!(s[1..].chars().all(|c| c.is_ascii_digit()), "{s}: bad number");
            assert!(numbers.insert(&s[1..]), "{s}: number used more than once");
            assert!(!summary.is_empty());
        }
    }

    #[test]
    fn every_code_is_documented() {
        let doc = include_str!("../DIAGNOSTICS.md");
        for (code, _) in ALL {
            assert!(doc.contains(code.as_str()), "{code} missing from DIAGNOSTICS.md");
        }
    }

    #[test]
    fn code_survives_context_layers() {
        use anyhow::Context;
        let e = error(codes::MESH_NOT_FOUND, "no mesh".into());
        let e = Err::<(), _>(e).context("outer").context("outermost").unwrap_err();
        assert_eq!(code_of(&e), codes::MESH_NOT_FOUND);
        assert_eq!(format!("{e:#}"), "outermost: outer: no mesh");
    }

    #[test]
    fn uncoded_errors_get_the_fallback_code() {
        assert_eq!(code_of(&anyhow::anyhow!("boom")), codes::UNCLASSIFIED);
    }
}
