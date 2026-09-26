//! Parsing of the `--vfmt` vertex format language (spec sections 9-15) and
//! computation of the resulting vertex layout.

use anyhow::{anyhow, bail, Result};
use std::collections::HashSet;

pub const HIDDEN_NAME: &str = "_";

/// One of the built-in glTF source bindings (spec section 11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Position,
    Normal,
    Tangent,
    Uv(u32),
}

impl SourceKind {
    /// Number of components this binding expects/produces.
    pub fn component_count(self) -> u32 {
        match self {
            SourceKind::Position => 3,
            SourceKind::Normal => 3,
            SourceKind::Tangent => 4,
            SourceKind::Uv(_) => 2,
        }
    }

    /// The glTF attribute semantic name this binding maps to (spec section 11).
    pub fn gltf_attribute_name(self) -> String {
        match self {
            SourceKind::Position => "POSITION".to_string(),
            SourceKind::Normal => "NORMAL".to_string(),
            SourceKind::Tangent => "TANGENT".to_string(),
            SourceKind::Uv(n) => format!("TEXCOORD_{n}"),
        }
    }

    fn parse(s: &str) -> Option<SourceKind> {
        match s {
            "position" => Some(SourceKind::Position),
            "normal" => Some(SourceKind::Normal),
            "tangent" => Some(SourceKind::Tangent),
            _ => {
                let n = s.strip_prefix("uv")?;
                if n.is_empty() || !n.chars().all(|c| c.is_ascii_digit()) {
                    return None;
                }
                Some(SourceKind::Uv(n.parse().ok()?))
            }
        }
    }
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceKind::Position => write!(f, "position"),
            SourceKind::Normal => write!(f, "normal"),
            SourceKind::Tangent => write!(f, "tangent"),
            SourceKind::Uv(n) => write!(f, "uv{n}"),
        }
    }
}

/// An application-filled type (spec section 12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppType {
    Float,
    Vec2,
    Vec3,
    Vec4,
}

impl AppType {
    pub fn component_count(self) -> u32 {
        match self {
            AppType::Float => 1,
            AppType::Vec2 => 2,
            AppType::Vec3 => 3,
            AppType::Vec4 => 4,
        }
    }

    pub fn size_bytes(self) -> u32 {
        self.component_count() * 4
    }

    fn parse(s: &str) -> Option<AppType> {
        match s {
            "float" => Some(AppType::Float),
            "vec2" => Some(AppType::Vec2),
            "vec3" => Some(AppType::Vec3),
            "vec4" => Some(AppType::Vec4),
            _ => None,
        }
    }
}

/// What a single vertex-format field binds to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    Source(SourceKind),
    App(AppType),
    Pad(u32),
}

/// A single parsed field of a `--vfmt` string, e.g. `position`, `uv/uv0`,
/// `_/pad4`, or `appData/vec4`.
#[derive(Debug, Clone)]
pub struct VfmtField {
    /// The raw emitted-attribute name as written (may be `_` for hidden).
    pub name: String,
    pub binding: Binding,
}

impl VfmtField {
    pub fn is_hidden(&self) -> bool {
        self.name == HIDDEN_NAME
    }
}

fn parse_binding(s: &str) -> Result<Binding> {
    if let Some(kind) = SourceKind::parse(s) {
        return Ok(Binding::Source(kind));
    }
    if let Some(app) = AppType::parse(s) {
        return Ok(Binding::App(app));
    }
    if let Some(rest) = s.strip_prefix("pad") {
        if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()) {
            let n: u32 = rest
                .parse()
                .map_err(|_| anyhow!("padding amount too large: pad{rest}"))?;
            if n == 0 {
                bail!("padding amount must be a positive integer, got pad{rest}");
            }
            return Ok(Binding::Pad(n));
        }
    }
    bail!(
        "unknown vertex format binding {s:?}: expected one of position, normal, tangent, uvN, \
         float, vec2, vec3, vec4, or padN"
    )
}

/// Parse a full `--vfmt` string into its fields, in order. Does not yet
/// validate against a particular primitive's available attributes; see
/// [`Layout::build`] for the remaining structural validation (duplicate
/// sources, hidden/padding rules) and [`crate::commands::mesh`] for
/// primitive-specific validation (attribute existence, float-ness, counts).
pub fn parse_vfmt(s: &str) -> Result<Vec<VfmtField>> {
    let mut fields = Vec::new();
    for raw_field in s.split(',') {
        let raw_field = raw_field.trim();
        if raw_field.is_empty() {
            bail!("empty field in vertex format: {s:?}");
        }
        let (name, binding_str) = match raw_field.split_once('/') {
            Some((name, binding)) => (name, binding),
            None => (raw_field, raw_field),
        };
        if name.is_empty() {
            bail!("empty attribute name in vertex format field: {raw_field:?}");
        }
        let binding = parse_binding(binding_str)
            .map_err(|e| anyhow!("in vertex format field {raw_field:?}: {e}"))?;

        if let Binding::Pad(_) = binding {
            if name != HIDDEN_NAME {
                bail!(
                    "padding (padN) must use the hidden attribute name '_', e.g. \"_/{binding_str}\", \
                     got: {raw_field:?}"
                );
            }
        }

        fields.push(VfmtField {
            name: name.to_string(),
            binding,
        });
    }
    if fields.is_empty() {
        bail!("vertex format must not be empty");
    }
    Ok(fields)
}

/// A single entry of the computed, in-order vertex layout.
#[derive(Debug, Clone)]
pub struct LayoutEntry {
    pub field: VfmtField,
    pub offset: u32,
    pub size: u32,
    pub component_count: u32,
    /// `true` if this entry produces an MDX1 vertex attribute definition.
    pub emits_attribute: bool,
}

impl LayoutEntry {
    pub fn source_kind(&self) -> Option<SourceKind> {
        match self.field.binding {
            Binding::Source(k) => Some(k),
            _ => None,
        }
    }
}

/// The fully computed vertex layout for a `--vfmt` string.
#[derive(Debug, Clone)]
pub struct Layout {
    pub entries: Vec<LayoutEntry>,
    pub vertex_size: u32,
}

impl Layout {
    /// Build and validate the layout implied by `fields` (spec sections
    /// 11-15): source bindings used at most once, and the general packing
    /// rule (consecutive, no implicit padding/alignment).
    pub fn build(fields: Vec<VfmtField>) -> Result<Layout> {
        let mut entries = Vec::with_capacity(fields.len());
        let mut offset: u32 = 0;
        let mut used_sources: HashSet<SourceKind> = HashSet::new();

        for field in fields {
            let (size, component_count, emits_attribute) = match field.binding {
                Binding::Source(kind) => {
                    if !used_sources.insert(kind) {
                        bail!(
                            "source binding '{kind}' is used more than once in the vertex format"
                        );
                    }
                    (kind.component_count() * 4, kind.component_count(), !field.is_hidden())
                }
                Binding::App(app) => (app.size_bytes(), app.component_count(), !field.is_hidden()),
                Binding::Pad(n) => (n, 0, false),
            };

            entries.push(LayoutEntry {
                field,
                offset,
                size,
                component_count,
                emits_attribute,
            });
            offset += size;
        }

        Ok(Layout {
            entries,
            vertex_size: offset,
        })
    }
}

/// Returns `true` if `name` looks like a valid GLSL variable identifier:
/// `[A-Za-z_][A-Za-z0-9_]*`, and not a reserved word that would be illegal
/// as a GLSL identifier. This is used only to produce a non-fatal warning
/// (spec section 10/22).
pub fn looks_like_glsl_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.clone().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    // GLSL identifiers must not start with "gl_", and reserved keywords are
    // not valid identifiers either.
    if name.starts_with("gl_") {
        return false;
    }
    !is_glsl_reserved(name)
}

fn is_glsl_reserved(name: &str) -> bool {
    const RESERVED: &[&str] = &[
        "attribute", "const", "uniform", "varying", "buffer", "shared", "coherent", "volatile",
        "restrict", "readonly", "writeonly", "layout", "centroid", "flat", "smooth",
        "noperspective", "patch", "sample", "invariant", "precise", "break", "continue", "do",
        "for", "while", "switch", "case", "default", "if", "else", "subroutine", "in", "out",
        "inout", "float", "double", "int", "void", "bool", "true", "false", "discard", "return",
        "struct", "vec2", "vec3", "vec4", "ivec2", "ivec3", "ivec4", "bvec2", "bvec3", "bvec4",
        "uint", "uvec2", "uvec3", "uvec4", "dvec2", "dvec3", "dvec4", "mat2", "mat3", "mat4",
        "precision", "highp", "mediump", "lowp", "sampler2D", "sampler3D", "samplerCube",
    ];
    RESERVED.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout() {
        let fields = parse_vfmt("position,normal,uv0").unwrap();
        let layout = Layout::build(fields).unwrap();
        assert_eq!(layout.vertex_size, 32);
        assert_eq!(layout.entries[0].offset, 0);
        assert_eq!(layout.entries[1].offset, 12);
        assert_eq!(layout.entries[2].offset, 24);
    }

    #[test]
    fn padding() {
        let fields = parse_vfmt("position,normal,_/pad4,uv0").unwrap();
        let layout = Layout::build(fields).unwrap();
        assert_eq!(layout.entries[2].offset, 24);
        assert_eq!(layout.entries[2].size, 4);
        assert!(!layout.entries[2].emits_attribute);
        assert_eq!(layout.entries[3].offset, 28);
        assert_eq!(layout.vertex_size, 36);
    }

    #[test]
    fn duplicate_source_is_error() {
        let fields = parse_vfmt("position,previousPosition/position").unwrap();
        assert!(Layout::build(fields).is_err());
    }

    #[test]
    fn named_pad_is_error() {
        assert!(parse_vfmt("foo/pad4").is_err());
    }

    #[test]
    fn app_filled() {
        let fields = parse_vfmt("position/position,appCalculatedAttribute/vec4,normal,uv/uv0")
            .unwrap();
        let layout = Layout::build(fields).unwrap();
        assert_eq!(layout.vertex_size, 12 + 16 + 12 + 8);
    }

    #[test]
    fn glsl_ident_check() {
        assert!(looks_like_glsl_identifier("a_position"));
        assert!(!looks_like_glsl_identifier("3d_position"));
        assert!(!looks_like_glsl_identifier("gl_Position"));
        assert!(!looks_like_glsl_identifier("uv0/bad"));
    }
}
