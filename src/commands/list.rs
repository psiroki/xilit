//! `list`, `mesh list`, `image list`, `material list`, `texture list`,
//! `node list`, `scene list` (spec section 3).

use crate::gltf_util::LoadedGltf;

fn display_name(name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{n:?}"),
        None => "<unnamed>".to_string(),
    }
}

/// `list` — a compact overview of available object types and counts.
pub fn overview(loaded: &LoadedGltf) {
    let doc = &loaded.document;
    println!("meshes: {}", doc.meshes().count());
    println!("images: {}", doc.images().count());
    println!("materials: {}", doc.materials().count());
    println!("textures: {}", doc.textures().count());
    println!("nodes: {}", doc.nodes().count());
    println!("scenes: {}", doc.scenes().count());
    let anim_count = doc.animations().count();
    if anim_count > 0 {
        println!("animations: {anim_count}");
    }
    let skin_count = doc.skins().count();
    if skin_count > 0 {
        println!("skins: {skin_count}");
    }
    let cam_count = doc.cameras().count();
    if cam_count > 0 {
        println!("cameras: {cam_count}");
    }
}

pub fn meshes(loaded: &LoadedGltf) {
    for mesh in loaded.document.meshes() {
        let prims = mesh.primitives().count();
        println!(
            "#{}: {} ({} primitive{})",
            mesh.index(),
            display_name(mesh.name()),
            prims,
            if prims == 1 { "" } else { "s" }
        );
        for prim in mesh.primitives() {
            let mode = format!("{:?}", prim.mode());
            let indexed = if prim.indices().is_some() { "indexed" } else { "non-indexed" };
            let attrs: Vec<String> = prim
                .attributes()
                .map(|(sem, _)| crate::commands::mesh::semantic_key(&sem))
                .collect();
            println!(
                "    [{}] mode={mode} {indexed} attributes=[{}]",
                prim.index(),
                attrs.join(", ")
            );
        }
    }
}

pub fn images(loaded: &LoadedGltf) {
    for image in loaded.document.images() {
        let source = match image.source() {
            gltf::image::Source::Uri { uri, .. } => format!("uri={uri}"),
            gltf::image::Source::View { .. } => "embedded (bufferView)".to_string(),
        };
        println!("#{}: {} ({source})", image.index(), display_name(image.name()));
    }
}

pub fn materials(loaded: &LoadedGltf) {
    for mat in loaded.document.materials() {
        let idx = mat
            .index()
            .map(|i| i.to_string())
            .unwrap_or_else(|| "default".to_string());
        println!("#{idx}: {}", display_name(mat.name()));
    }
}

pub fn textures(loaded: &LoadedGltf) {
    for tex in loaded.document.textures() {
        println!(
            "#{}: {} (image #{})",
            tex.index(),
            display_name(tex.name()),
            tex.source().index()
        );
    }
}

pub fn nodes(loaded: &LoadedGltf) {
    for node in loaded.document.nodes() {
        let mesh = node
            .mesh()
            .map(|m| format!(" mesh=#{}", m.index()))
            .unwrap_or_default();
        let children = node.children().count();
        println!(
            "#{}: {}{mesh} ({children} child{})",
            node.index(),
            display_name(node.name()),
            if children == 1 { "" } else { "ren" }
        );
    }
}

pub fn scenes(loaded: &LoadedGltf) {
    for scene in loaded.document.scenes() {
        println!(
            "#{}: {} ({} root node{})",
            scene.index(),
            display_name(scene.name()),
            scene.nodes().count(),
            if scene.nodes().count() == 1 { "" } else { "s" }
        );
    }
}
