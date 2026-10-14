//! `--matmap` (spec 2.0 section 5) and the `MATL` payload (section 7).
//!
//! A mapping reads `<uniform>:<type>/<path>`. The path is a dot-notation
//! query into the glTF material *as a JSON object with defaults filled in*
//! (e.g. `pbrMetallicRoughness.baseColorFactor` is always present, an unset
//! `normalTexture` is not). Numeric path segments index into arrays, so
//! `pbrMetallicRoughness.baseColorFactor.3` is the alpha component.
//!
//! MATL payload (all `u32` little-endian): shader name (string pool offset),
//! uniform count, then per uniform: name (string pool offset), GL type
//! (0 = null), size in 32-bit words, value words. Floats are raw `f32`
//! bits; a `sampler2D` is a string pool offset (top 4 bits = flags, 0) of
//! the texture's image path.

use crate::diag::codes;
use crate::gltf_util::percent_decode;
use crate::mdx2::StringPool;
use crate::vfmt::looks_like_glsl_identifier;
use anyhow::Result;
use gltf::json::Value;
use gltf::Document;
use std::collections::HashSet;

const GL_FLOAT: u32 = 0x1406;
const GL_FLOAT_VEC2: u32 = 0x8B50;
const GL_FLOAT_VEC3: u32 = 0x8B51;
const GL_FLOAT_VEC4: u32 = 0x8B52;
const GL_SAMPLER_2D: u32 = 0x8B5E;
const GL_NULL: u32 = 0;

/// Sampler values keep flags in the top 4 bits, so offsets must fit in 28.
const SAMPLER_OFFSET_LIMIT: u32 = 1 << 28;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Sampler2D,
}

impl UType {
    fn parse(s: &str) -> Option<UType> {
        match s {
            "float" => Some(UType::Float),
            "vec2" => Some(UType::Vec2),
            "vec3" => Some(UType::Vec3),
            "vec4" => Some(UType::Vec4),
            "sampler2D" => Some(UType::Sampler2D),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            UType::Float => "float",
            UType::Vec2 => "vec2",
            UType::Vec3 => "vec3",
            UType::Vec4 => "vec4",
            UType::Sampler2D => "sampler2D",
        }
    }

    fn gl(self) -> u32 {
        match self {
            UType::Float => GL_FLOAT,
            UType::Vec2 => GL_FLOAT_VEC2,
            UType::Vec3 => GL_FLOAT_VEC3,
            UType::Vec4 => GL_FLOAT_VEC4,
            UType::Sampler2D => GL_SAMPLER_2D,
        }
    }

    /// Number of float components, for the numeric types.
    fn components(self) -> usize {
        match self {
            UType::Float => 1,
            UType::Vec2 => 2,
            UType::Vec3 => 3,
            UType::Vec4 => 4,
            UType::Sampler2D => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mapping {
    pub name: String,
    pub utype: UType,
    pub path: String,
}

/// Parse a `--matmap` value: comma-separated `name:type/path` entries.
pub fn parse_matmap(s: &str) -> Result<Vec<Mapping>> {
    let mut mappings = Vec::new();
    let mut seen = HashSet::new();

    for entry in s.split(',') {
        let entry = entry.trim();
        let Some((name, rest)) = entry.split_once(':') else {
            bail_code!(
                codes::MATMAP_SYNTAX,
                "matmap entry {entry:?}: expected <uniform>:<type>/<path>"
            );
        };
        let Some((type_str, path)) = rest.split_once('/') else {
            bail_code!(
                codes::MATMAP_SYNTAX,
                "matmap entry {entry:?}: expected <uniform>:<type>/<path>"
            );
        };
        if name.is_empty() {
            bail_code!(codes::MATMAP_SYNTAX, "matmap entry {entry:?}: empty uniform name");
        }
        if path.is_empty() || path.split('.').any(str::is_empty) {
            bail_code!(
                codes::MATMAP_SYNTAX,
                "matmap entry {entry:?}: the path must be non-empty dot-separated segments"
            );
        }
        let Some(utype) = UType::parse(type_str) else {
            bail_code!(
                codes::MATMAP_UNKNOWN_TYPE,
                "matmap entry {entry:?}: unknown type {type_str:?} (expected float, vec2, vec3, vec4 or sampler2D)"
            );
        };
        if !seen.insert(name.to_string()) {
            bail_code!(
                codes::MATMAP_DUPLICATE_UNIFORM,
                "uniform {name:?} is mapped more than once"
            );
        }
        if !looks_like_glsl_identifier(name) {
            warn_code!(
                codes::MATMAP_BAD_GLSL_NAME,
                "uniform name {name:?} does not look like a valid GLSL variable identifier"
            );
        }
        mappings.push(Mapping {
            name: name.to_string(),
            utype,
            path: path.to_string(),
        });
    }

    Ok(mappings)
}

fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for segment in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    if current.is_null() {
        None
    } else {
        Some(current)
    }
}

/// The path to emit for texture `index`: its image's (percent-decoded) uri,
/// or the image's name when the image is embedded.
fn texture_path(doc: &Document, index: u64, label: &str) -> Result<String> {
    let json = doc.as_json();
    let Some(texture) = usize::try_from(index).ok().and_then(|i| json.textures.get(i)) else {
        bail_code!(
            codes::TEXTURE_NOT_FOUND,
            "{label}: texture #{index} does not exist ({} textures)",
            json.textures.len()
        );
    };

    // `source`, or — when absent, as with EXT_texture_webp and friends — the
    // `source` of the first extension that has one.
    let core = texture.source.value();
    let image_index = if core < json.images.len() {
        Some(core)
    } else {
        gltf::json::serialize::to_value(texture)
            .ok()
            .and_then(|v| match v.get("extensions") {
                Some(Value::Object(exts)) => exts
                    .values()
                    .find_map(|ext| ext.get("source").and_then(Value::as_u64)),
                _ => None,
            })
            .and_then(|i| usize::try_from(i).ok())
    };
    let Some(image) = image_index.and_then(|i| json.images.get(i)) else {
        bail_code!(
            codes::TEXTURE_NO_SOURCE,
            "{label}: texture #{index} has no usable image source"
        );
    };

    match (&image.uri, &image.name) {
        (Some(uri), _) if !uri.starts_with("data:") => Ok(percent_decode(uri)),
        (_, Some(name)) => Ok(name.clone()),
        _ => bail_code!(
            codes::IMAGE_NO_PATH,
            "{label}: the image of texture #{index} is embedded and unnamed, so it has no path"
        ),
    }
}

fn push(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Build the `MATL` payload for glTF material `material` (`None` = the
/// implicit default material used by primitives without one).
pub fn build_matl(
    doc: &Document,
    material: Option<usize>,
    mappings: &[Mapping],
    pool: &mut StringPool,
) -> Result<Vec<u8>> {
    let json = doc.as_json();
    let default_material = gltf::json::Material::default();
    let (mat, label) = match material {
        Some(i) => {
            let m = &json.materials[i];
            let label = match &m.name {
                Some(n) => format!("material #{i} {n:?}"),
                None => format!("material #{i}"),
            };
            (m, label)
        }
        None => (&default_material, "the default material".to_string()),
    };

    let shader = match &mat.name {
        Some(n) => n.clone(),
        None => {
            warn_code!(
                codes::UNNAMED_MATERIAL,
                "{label} has no name; its shader name will be empty"
            );
            String::new()
        }
    };

    let value = gltf::json::serialize::to_value(mat).map_err(|e| {
        err_code!(codes::MATERIAL_UNREADABLE, "{label} could not be read: {e}")
    })?;

    let mut out = Vec::new();
    push(&mut out, pool.intern(&shader)?);
    push(&mut out, mappings.len() as u32);

    for m in mappings {
        let name = pool.intern(&m.name)?;
        push(&mut out, name);

        let Some(found) = lookup(&value, &m.path) else {
            warn_code!(
                codes::MATMAP_MISSING_VALUE,
                "{label}: nothing at '{}' for uniform '{}'; encoded as null",
                m.path,
                m.name
            );
            push(&mut out, GL_NULL);
            push(&mut out, 0);
            continue;
        };

        let mismatch = || {
            err_code!(
                codes::MATMAP_TYPE_MISMATCH,
                "{label}: '{}' resolves to {found}, which is not a valid {} for uniform '{}'",
                m.path,
                m.utype.name(),
                m.name
            )
        };

        push(&mut out, m.utype.gl());
        match m.utype {
            UType::Sampler2D => {
                let index = found.as_u64().ok_or_else(mismatch)?;
                let offset = pool.intern(&texture_path(doc, index, &label)?)?;
                if offset >= SAMPLER_OFFSET_LIMIT {
                    bail_code!(
                        codes::STRING_POOL_TOO_LARGE,
                        "string pool offset {offset} does not fit the 28-bit sampler field"
                    );
                }
                push(&mut out, 1);
                push(&mut out, offset);
            }
            numeric => {
                let n = numeric.components();
                let numbers: Vec<f64> = match found {
                    Value::Array(items) if n > 1 => items
                        .iter()
                        .map(|v| v.as_f64())
                        .collect::<Option<Vec<_>>>()
                        .ok_or_else(mismatch)?,
                    other if n == 1 => vec![other.as_f64().ok_or_else(mismatch)?],
                    _ => return Err(mismatch()),
                };
                if numbers.len() != n {
                    return Err(mismatch());
                }
                push(&mut out, n as u32);
                for v in numbers {
                    push(&mut out, (v as f32).to_bits());
                }
            }
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODEL: &str = r#"{
        "asset": {"version": "2.0"},
        "images": [{"uri": "tex/wood%20grain.png"}, {"name": "embedded_name", "bufferView": 0}, {"bufferView": 0}],
        "textures": [{"source": 0}, {"source": 1}, {"source": 2}, {"extensions": {"EXT_texture_webp": {"source": 0}}}],
        "materials": [
            {"name": "wood",
             "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.25,
                                      "baseColorFactor": [0.5, 0.25, 1.0, 0.75]},
             "normalTexture": {"index": 1}},
            {"name": "plain"},
            {"pbrMetallicRoughness": {"baseColorTexture": {"index": 2}}},
            {"name": "webp", "pbrMetallicRoughness": {"baseColorTexture": {"index": 3}}}
        ]
    }"#;

    fn d() -> Document {
        gltf::Gltf::from_slice_without_validation(MODEL.as_bytes()).unwrap().document
    }

    fn words(b: &[u8]) -> Vec<u32> {
        b.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
    }

    fn pool_string(pool: &StringPool, at: u32) -> String {
        let tail = &pool.bytes()[at as usize..];
        String::from_utf8(tail[..tail.iter().position(|&b| b == 0).unwrap()].to_vec()).unwrap()
    }

    #[test]
    fn parses_the_spec_example() {
        let m = parse_matmap(
            "albedo:sampler2D/pbrMetallicRoughness.baseColorTexture.index,normal:sampler2D/normalTexture.index",
        )
        .unwrap();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].name, "albedo");
        assert_eq!(m[0].utype, UType::Sampler2D);
        assert_eq!(m[1].path, "normalTexture.index");
    }

    #[test]
    fn rejects_bad_syntax_types_and_duplicates() {
        let code = |s: &str| crate::diag::code_of(&parse_matmap(s).unwrap_err());
        assert_eq!(code("albedo"), codes::MATMAP_SYNTAX);
        assert_eq!(code("albedo:float"), codes::MATMAP_SYNTAX);
        assert_eq!(code(":float/a"), codes::MATMAP_SYNTAX);
        assert_eq!(code("a:float/x..y"), codes::MATMAP_SYNTAX);
        assert_eq!(code("a:float/"), codes::MATMAP_SYNTAX);
        assert_eq!(code("a:mat4/x"), codes::MATMAP_UNKNOWN_TYPE);
        assert_eq!(code("a:float/x,a:vec2/y"), codes::MATMAP_DUPLICATE_UNIFORM);
    }

    #[test]
    fn encodes_textures_and_numbers() {
        let doc = d();
        let maps = parse_matmap(
            "albedo:sampler2D/pbrMetallicRoughness.baseColorTexture.index,\
             metal:float/pbrMetallicRoughness.metallicFactor,\
             tint:vec4/pbrMetallicRoughness.baseColorFactor,\
             alpha:float/pbrMetallicRoughness.baseColorFactor.3",
        )
        .unwrap();
        let mut pool = StringPool::new();
        let w = words(&build_matl(&doc, Some(0), &maps, &mut pool).unwrap());

        assert_eq!(pool_string(&pool, w[0]), "wood");
        assert_eq!(w[1], 4);
        // albedo: sampler2D, 1 word, pool offset of the decoded path
        assert_eq!(pool_string(&pool, w[2]), "albedo");
        assert_eq!((w[3], w[4]), (GL_SAMPLER_2D, 1));
        assert_eq!(pool_string(&pool, w[5]), "tex/wood grain.png");
        // metal: float 0.25
        assert_eq!((w[7], w[8]), (GL_FLOAT, 1));
        assert_eq!(f32::from_bits(w[9]), 0.25);
        // tint: vec4
        assert_eq!((w[11], w[12]), (GL_FLOAT_VEC4, 4));
        let tint: Vec<f32> = w[13..17].iter().map(|&b| f32::from_bits(b)).collect();
        assert_eq!(tint, [0.5, 0.25, 1.0, 0.75]);
        // alpha: float from an array element
        assert_eq!((w[18], w[19]), (GL_FLOAT, 1));
        assert_eq!(f32::from_bits(w[20]), 0.75);
        assert_eq!(w.len(), 21);
    }

    #[test]
    fn defaults_are_filled_in() {
        // "plain" sets nothing, yet the standard defaults are addressable.
        let maps = parse_matmap("c:vec4/pbrMetallicRoughness.baseColorFactor,r:float/pbrMetallicRoughness.roughnessFactor").unwrap();
        let mut pool = StringPool::new();
        let w = words(&build_matl(&d(), Some(1), &maps, &mut pool).unwrap());
        assert_eq!(f32::from_bits(w[7]), 1.0);
        assert_eq!(f32::from_bits(w[w.len() - 1]), 1.0);
    }

    #[test]
    fn missing_values_become_null_uniforms() {
        let maps = parse_matmap("normal:sampler2D/normalTexture.index").unwrap();
        let mut pool = StringPool::new();
        let w = words(&build_matl(&d(), Some(1), &maps, &mut pool).unwrap());
        assert_eq!(w.len(), 5);
        assert_eq!((w[3], w[4]), (GL_NULL, 0));
        assert_eq!(pool_string(&pool, w[2]), "normal");
    }

    #[test]
    fn default_and_unnamed_materials_get_an_empty_shader_name() {
        let mut pool = StringPool::new();
        let w = words(&build_matl(&d(), None, &[], &mut pool).unwrap());
        assert_eq!(pool_string(&pool, w[0]), "");
        assert_eq!(w[1], 0);
        let w = words(&build_matl(&d(), Some(2), &[], &mut pool).unwrap());
        assert_eq!(pool_string(&pool, w[0]), "");
    }

    #[test]
    fn embedded_images_fall_back_to_their_name_or_fail() {
        let maps = parse_matmap("normal:sampler2D/normalTexture.index").unwrap();
        let mut pool = StringPool::new();
        let w = words(&build_matl(&d(), Some(0), &maps, &mut pool).unwrap());
        assert_eq!(pool_string(&pool, w[5]), "embedded_name");

        let maps = parse_matmap("a:sampler2D/pbrMetallicRoughness.baseColorTexture.index").unwrap();
        let e = build_matl(&d(), Some(2), &maps, &mut pool).unwrap_err();
        assert_eq!(crate::diag::code_of(&e), codes::IMAGE_NO_PATH);
    }

    #[test]
    fn extension_provided_texture_sources_are_followed() {
        let maps = parse_matmap("a:sampler2D/pbrMetallicRoughness.baseColorTexture.index").unwrap();
        let mut pool = StringPool::new();
        let w = words(&build_matl(&d(), Some(3), &maps, &mut pool).unwrap());
        assert_eq!(pool_string(&pool, w[5]), "tex/wood grain.png");
    }

    #[test]
    fn type_mismatches_are_errors() {
        let mut pool = StringPool::new();
        let code = |m: &str, mat: usize, pool: &mut StringPool| {
            let maps = parse_matmap(m).unwrap();
            crate::diag::code_of(&build_matl(&d(), Some(mat), &maps, pool).unwrap_err())
        };
        // a vec4 is not a float, a float is not a vec3, a name is not a texture
        assert_eq!(code("a:float/pbrMetallicRoughness.baseColorFactor", 0, &mut pool), codes::MATMAP_TYPE_MISMATCH);
        assert_eq!(code("a:vec3/pbrMetallicRoughness.baseColorFactor", 0, &mut pool), codes::MATMAP_TYPE_MISMATCH);
        assert_eq!(code("a:vec3/pbrMetallicRoughness.metallicFactor", 0, &mut pool), codes::MATMAP_TYPE_MISMATCH);
        assert_eq!(code("a:sampler2D/name", 0, &mut pool), codes::MATMAP_TYPE_MISMATCH);
    }

    #[test]
    fn unknown_texture_index_is_an_error() {
        let doc = gltf::Gltf::from_slice_without_validation(
            br#"{"asset":{"version":"2.0"},"materials":[{"name":"m","normalTexture":{"index":7}}]}"#,
        )
        .unwrap()
        .document;
        let maps = parse_matmap("n:sampler2D/normalTexture.index").unwrap();
        let e = build_matl(&doc, Some(0), &maps, &mut StringPool::new()).unwrap_err();
        assert_eq!(crate::diag::code_of(&e), codes::TEXTURE_NOT_FOUND);
    }
}
