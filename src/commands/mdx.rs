//! `mdx export <selector>... <filename.mdx> [--vfmt ..] [--matmap ..] [--no-descendants]`
//! (spec 2.0 sections 2-5).

use crate::commands::mesh::semantic_key;
use crate::diag::codes;
use crate::gltf_util::{self, LoadedGltf};
use crate::matmap;
use crate::mdx2::{self, StringPool};
use crate::objref::unquote_token;
use crate::scene::Hierarchy;
use crate::selector::{self, Selector};
use crate::vfmt::{self, Binding, Layout, SourceKind};
use crate::xform::{self, Xform};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;

pub const DEFAULT_VFMT: &str = "position,normal,uv0";

/// Largest vertex/index count of one batch (spec 2.0 sections 1 and 4).
const MAX_BATCH: usize = 65_535;

/// Batch key of primitives without a material (sorts after all real ones).
const DEFAULT_MATERIAL: usize = usize::MAX;

const USAGE: &str = "usage: mdx export <selector>... <filename.mdx> \
                     [--vfmt <vertexFormat>] [--matmap <mapping>] [--no-descendants]";

struct ExportArgs {
    selectors: Vec<Selector>,
    out_path: String,
    vfmt: String,
    matmap: Option<String>,
    descendants: bool,
}

fn parse_args(args: &[String]) -> Result<ExportArgs> {
    let mut positional: Vec<&String> = Vec::new();
    let mut vfmt = DEFAULT_VFMT.to_string();
    let mut matmap = None;
    let mut descendants = true;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let mut value = |name: &str| -> Result<String> {
            i += 1;
            let v = args.get(i).ok_or_else(|| {
                err_code!(codes::MISSING_OPTION_VALUE, "{name} requires a value")
            })?;
            unquote_token(v)
        };
        match arg {
            "--vfmt" => vfmt = value("--vfmt")?,
            "--matmap" => matmap = Some(value("--matmap")?),
            "--no-descendants" => descendants = false,
            other if other.starts_with("--") => {
                bail_code!(codes::UNEXPECTED_ARGUMENT, "unexpected option: {other}")
            }
            _ => positional.push(&args[i]),
        }
        i += 1;
    }

    if positional.len() < 2 {
        bail_code!(codes::BAD_USAGE, "{USAGE}");
    }
    let out_path = unquote_token(positional[positional.len() - 1])?;
    let selectors = positional[..positional.len() - 1]
        .iter()
        .map(|tok| selector::parse_selector(tok))
        .collect::<Result<Vec<_>>>()?;

    Ok(ExportArgs {
        selectors,
        out_path,
        vfmt,
        matmap,
        descendants,
    })
}

/// A primitive's source data, read and validated once and then instanced.
struct PrimitiveData {
    vertex_count: usize,
    indices: Vec<u32>,
    sources: HashMap<SourceKind, Vec<Vec<f32>>>,
}

fn load_primitive(
    loaded: &LoadedGltf,
    mesh: &gltf::Mesh,
    prim: &gltf::Primitive,
    layout: &Layout,
) -> Result<PrimitiveData> {
    let label = format!("primitive #{}/{}", mesh.index(), prim.index());

    if prim.mode() != gltf::mesh::Mode::Triangles {
        bail_code!(
            codes::PRIMITIVE_NOT_TRIANGLES,
            "{label} uses mode {:?}; only TRIANGLES can be exported",
            prim.mode()
        );
    }
    let indices_accessor = prim.indices().ok_or_else(|| {
        err_code!(
            codes::PRIMITIVE_NOT_INDEXED,
            "{label} has no index accessor (non-indexed primitives are not supported)"
        )
    })?;
    let indices = gltf_util::read_index_accessor(&loaded.buffers, &indices_accessor)?;
    if indices.len() % 3 != 0 {
        bail_code!(
            codes::INDEX_COUNT_NOT_TRIANGLES,
            "{label} has {} indices, which is not a multiple of 3",
            indices.len()
        );
    }

    let attributes: HashMap<String, gltf::Accessor> = prim
        .attributes()
        .map(|(sem, acc)| (semantic_key(&sem), acc))
        .collect();
    let vertex_count = attributes
        .get("POSITION")
        .ok_or_else(|| err_code!(codes::MISSING_POSITION, "{label} has no POSITION attribute"))?
        .count();

    if let Some(bad) = indices.iter().find(|&&i| i as usize >= vertex_count) {
        bail_code!(
            codes::INDEX_OUT_OF_RANGE,
            "{label} has index {bad}, but only {vertex_count} vertices"
        );
    }

    // Everything the vertex format imports, plus NORMAL when tangents have
    // to be orthogonalized against it.
    let mut needed: Vec<SourceKind> = layout.entries.iter().filter_map(|e| e.source_kind()).collect();
    if needed.contains(&SourceKind::Tangent) && !needed.contains(&SourceKind::Normal) {
        if !attributes.contains_key("NORMAL") {
            bail_code!(
                codes::SOURCE_MISSING,
                "vertex format requests 'tangent', which needs the NORMAL attribute to orthogonalize \
                 against, but {label} has none"
            );
        }
        needed.push(SourceKind::Normal);
    }

    let mut sources = HashMap::new();
    for kind in needed {
        let attr_name = kind.gltf_attribute_name();
        let accessor = attributes.get(&attr_name).ok_or_else(|| {
            err_code!(
                codes::SOURCE_MISSING,
                "vertex format requests '{kind}', but {label} has no {attr_name} attribute"
            )
        })?;
        let rows = gltf_util::read_float_accessor(&loaded.buffers, accessor)
            .with_context(|| format!("reading source attribute '{kind}' ({attr_name}) of {label}"))?;
        if rows.len() != vertex_count {
            bail_code!(
                codes::SOURCE_COUNT_MISMATCH,
                "source attribute '{kind}' ({attr_name}) of {label} has {} vertices, but POSITION has {vertex_count}",
                rows.len()
            );
        }
        let expected = kind.component_count() as usize;
        if rows.first().map_or(false, |row| row.len() != expected) {
            bail_code!(
                codes::SOURCE_COMPONENTS_MISMATCH,
                "source attribute '{kind}' ({attr_name}) of {label} has {} components per vertex, expected {expected}",
                rows[0].len()
            );
        }
        sources.insert(kind, rows);
    }

    if prim.morph_targets().count() > 0 {
        warn_code!(
            codes::MORPH_TARGETS_IGNORED,
            "{label} has morph targets, which are not exported"
        );
    }

    Ok(PrimitiveData {
        vertex_count,
        indices,
        sources,
    })
}

/// Vertices and indices of one material batch, built up instance by instance.
#[derive(Default)]
struct BatchBuilder {
    vertex_count: usize,
    vertex_data: Vec<u8>,
    indices: Vec<u16>,
}

fn vec3(row: &[f32]) -> [f32; 3] {
    [row[0], row[1], row[2]]
}

impl BatchBuilder {
    /// Append one instance of `prim`, with `world` baked into its vertices.
    fn append(
        &mut self,
        layout: &Layout,
        prim: &PrimitiveData,
        world: &xform::Mat4,
        material_label: &str,
    ) -> Result<()> {
        let vertices = self.vertex_count + prim.vertex_count;
        if vertices > MAX_BATCH {
            bail_code!(
                codes::BATCH_TOO_MANY_VERTICES,
                "the batch for {material_label} would have {vertices} vertices; the limit is {MAX_BATCH}"
            );
        }
        let indices = self.indices.len() + prim.indices.len();
        if indices > MAX_BATCH {
            bail_code!(
                codes::BATCH_TOO_MANY_INDICES,
                "the batch for {material_label} would have {indices} indices; the limit is {MAX_BATCH}"
            );
        }
        if (vertices as u64) * u64::from(layout.vertex_size) > u64::from(u32::MAX) {
            bail_code!(
                codes::OUTPUT_TOO_LARGE,
                "the vertex buffer of the batch for {material_label} would exceed 4 GiB"
            );
        }

        let xf = Xform::new(*world);
        let base = self.vertex_count;
        let normals = prim.sources.get(&SourceKind::Normal);
        let stride = layout.vertex_size as usize;
        self.vertex_data.reserve(prim.vertex_count * stride);

        for v in 0..prim.vertex_count {
            let world_normal = normals.map(|rows| xf.normal(vec3(&rows[v])));
            for entry in &layout.entries {
                let Binding::Source(kind) = entry.field.binding else {
                    // application-filled attributes and padding are zero
                    self.vertex_data.resize(self.vertex_data.len() + entry.size as usize, 0);
                    continue;
                };
                let row = &prim.sources[&kind][v];
                let out: Vec<f32> = match kind {
                    SourceKind::Position => xf.point(vec3(row)).to_vec(),
                    SourceKind::Normal => world_normal.expect("normals were loaded").to_vec(),
                    SourceKind::Tangent => {
                        let n = world_normal.expect("normals were loaded");
                        let t = xf.tangent(vec3(row), n);
                        vec![t[0], t[1], t[2], row[3]]
                    }
                    SourceKind::Uv(_) => row.clone(),
                };
                for f in out {
                    self.vertex_data.extend_from_slice(&f.to_le_bytes());
                }
            }
        }

        // Cannot overflow: every index < prim.vertex_count and the total
        // vertex count was checked against MAX_BATCH above.
        self.indices
            .extend(prim.indices.iter().map(|&i| (base + i as usize) as u16));
        self.vertex_count = vertices;
        Ok(())
    }
}

fn material_label(doc: &gltf::Document, key: usize) -> String {
    if key == DEFAULT_MATERIAL {
        return "the default material".to_string();
    }
    match &doc.as_json().materials[key].name {
        Some(n) => format!("material #{key} {n:?}"),
        None => format!("material #{key}"),
    }
}

pub fn export(loaded: &LoadedGltf, args: &[String]) -> Result<()> {
    let args = parse_args(args)?;
    let doc = &loaded.document;

    let fields = vfmt::parse_vfmt(&args.vfmt)?;
    for field in &fields {
        if !field.is_hidden() && !vfmt::looks_like_glsl_identifier(&field.name) {
            warn_code!(
                codes::VFMT_BAD_GLSL_NAME,
                "emitted attribute name {:?} does not look like a valid GLSL variable identifier",
                field.name
            );
        }
    }
    let layout = Layout::build(fields)?;
    let mappings = match &args.matmap {
        Some(m) => matmap::parse_matmap(m)?,
        None => Vec::new(),
    };

    let hierarchy = Hierarchy::new(doc);
    let selected = selector::resolve(doc, &hierarchy, &args.selectors, args.descendants);

    // Bake every selected mesh instance into its material's batch.
    let mut cache: HashMap<(usize, usize), PrimitiveData> = HashMap::new();
    let mut batches: BTreeMap<usize, BatchBuilder> = BTreeMap::new();
    let mut instances = 0usize;

    for &node_index in &selected {
        let node = doc.nodes().nth(node_index).expect("selected nodes exist");
        let Some(mesh) = node.mesh() else {
            continue;
        };

        let world = hierarchy.world(node_index)?;
        let det = xform::linear_det(&world);
        let node_label = match node.name() {
            Some(n) => format!("node #{node_index} {n:?}"),
            None => format!("node #{node_index}"),
        };
        if det < 0.0 {
            bail_code!(
                codes::NEGATIVE_DETERMINANT,
                "the world transform of {node_label} mirrors geometry (negative determinant); \
                 reflections and negative scales are not supported"
            );
        }
        if det == 0.0 || !det.is_finite() {
            bail_code!(
                codes::SINGULAR_TRANSFORM,
                "the world transform of {node_label} is degenerate (determinant {det})"
            );
        }
        if node.skin().is_some() {
            warn_code!(
                codes::SKIN_IGNORED,
                "{node_label} has a skin; its geometry is exported unskinned"
            );
        }

        for prim in mesh.primitives() {
            let key = (mesh.index(), prim.index());
            if !cache.contains_key(&key) {
                let data = load_primitive(loaded, &mesh, &prim, &layout)?;
                cache.insert(key, data);
            }
            let material = prim.material().index().unwrap_or(DEFAULT_MATERIAL);
            batches.entry(material).or_default().append(
                &layout,
                &cache[&key],
                &world,
                &material_label(doc, material),
            )?;
            instances += 1;
        }
    }

    if batches.is_empty() {
        bail_code!(
            codes::NO_GEOMETRY,
            "the selection contains no geometry to export ({} node(s) selected, none with a mesh)",
            selected.len()
        );
    }

    let mut pool = StringPool::new();
    let mut out_batches = Vec::with_capacity(batches.len());
    let (mut total_vertices, mut total_indices) = (0usize, 0usize);
    for (&key, batch) in &batches {
        let material = (key != DEFAULT_MATERIAL).then_some(key);
        out_batches.push(mdx2::Batch {
            vertex_count: batch.vertex_count as u32,
            vertex_data: batch.vertex_data.clone(),
            indices: batch.indices.clone(),
            material: matmap::build_matl(doc, material, &mappings, &mut pool)?,
        });
        total_vertices += batch.vertex_count;
        total_indices += batch.indices.len();
    }

    let bytes = mdx2::write(&layout, pool, &out_batches)?;

    let mut file = std::fs::File::create(&args.out_path).map_err(|e| {
        err_code!(
            codes::OUTPUT_CREATE_FAILED,
            "failed to create output file {}: {e}",
            args.out_path
        )
    })?;
    file.write_all(&bytes).map_err(|e| {
        err_code!(
            codes::OUTPUT_WRITE_FAILED,
            "failed to write output file {}: {e}",
            args.out_path
        )
    })?;

    println!(
        "exported {instances} mesh instance(s) from {} node(s) -> {} \
         ({} batch(es), {total_vertices} vertices, {total_indices} indices, {} bytes)",
        selected.len(),
        args.out_path,
        out_batches.len(),
        bytes.len()
    );
    Ok(())
}
