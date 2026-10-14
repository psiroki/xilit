//! Helpers shared by the mesh-related commands. (Primitive-level MDX1 export
//! was removed in xilit 2.0; see `commands::mdx` for the model-level export.)

/// Map a glTF attribute [`gltf::Semantic`] to its on-the-wire attribute
/// name (`POSITION`, `TEXCOORD_0`, ...).
pub fn semantic_key(sem: &gltf::Semantic) -> String {
    use gltf::Semantic;
    match sem {
        Semantic::Positions => "POSITION".to_string(),
        Semantic::Normals => "NORMAL".to_string(),
        Semantic::Tangents => "TANGENT".to_string(),
        Semantic::TexCoords(n) => format!("TEXCOORD_{n}"),
        Semantic::Colors(n) => format!("COLOR_{n}"),
        Semantic::Joints(n) => format!("JOINTS_{n}"),
        Semantic::Weights(n) => format!("WEIGHTS_{n}"),
        Semantic::Extras(name) => name.clone(),
    }
}
