//! `mdx export` object selectors (spec 2.0 section 3).
//!
//! A selector is an object reference (`Name` / `"Quoted Name"` / `#index`,
//! see [`crate::objref`]) where a name is a glob pattern (`*`, `?`, `[..]`),
//! optionally prefixed by an object kind:
//!
//! | selector          | meaning                                                    |
//! |-------------------|------------------------------------------------------------|
//! | `node:<ref>`      | matching nodes                                             |
//! | `mesh:<ref>`      | every node that instantiates a matching mesh               |
//! | `scene:<ref>`     | the root nodes of matching scenes                          |
//! | `<pattern>`       | nodes, meshes *and* scenes whose name matches              |
//! | `#N`              | node N (use a prefix for mesh/scene indices)               |
//!
//! Everything resolves to a set of node indices; a node selected several
//! ways is exported once.

use crate::diag::codes;
use crate::objref::{parse_objref, ObjRef};
use crate::scene::Hierarchy;
use anyhow::{Context, Result};
use gltf::Document;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Any,
    Node,
    Mesh,
    Scene,
}

#[derive(Debug)]
enum Matcher {
    Index(usize),
    Pattern(glob::Pattern),
}

impl Matcher {
    fn matches(&self, index: usize, name: Option<&str>) -> bool {
        match self {
            Matcher::Index(i) => *i == index,
            Matcher::Pattern(p) => name.map_or(false, |n| p.matches(n)),
        }
    }

    fn is_pattern(&self) -> bool {
        matches!(self, Matcher::Pattern(_))
    }
}

#[derive(Debug)]
pub struct Selector {
    kind: Kind,
    matcher: Matcher,
    raw: String,
}

/// Parse one raw (still quoted) selector token.
pub fn parse_selector(tok: &str) -> Result<Selector> {
    let (kind, rest) = if let Some(r) = tok.strip_prefix("node:") {
        (Kind::Node, r)
    } else if let Some(r) = tok.strip_prefix("mesh:") {
        (Kind::Mesh, r)
    } else if let Some(r) = tok.strip_prefix("scene:") {
        (Kind::Scene, r)
    } else {
        (Kind::Any, tok)
    };

    let target = parse_objref(rest).with_context(|| format!("invalid selector {tok}"))?;
    let matcher = match target {
        ObjRef::Index(i) => Matcher::Index(i),
        ObjRef::Name(pattern) => {
            let compiled = glob::Pattern::new(&pattern).map_err(|e| {
                err_code!(
                    codes::SELECTOR_INVALID_PATTERN,
                    "invalid glob pattern {pattern:?} in selector {tok}: {e}"
                )
            })?;
            Matcher::Pattern(compiled)
        }
    };

    Ok(Selector {
        kind,
        matcher,
        raw: tok.to_string(),
    })
}

/// Resolve `selectors` to the set of selected node indices. Selectors that
/// match no object at all produce a non-fatal warning.
pub fn resolve(
    doc: &Document,
    hierarchy: &Hierarchy,
    selectors: &[Selector],
    descendants: bool,
) -> BTreeSet<usize> {
    let mut selected = BTreeSet::new();

    let add_node = |node: usize, selected: &mut BTreeSet<usize>| {
        if descendants {
            hierarchy.insert_with_descendants(node, selected);
        } else {
            selected.insert(node);
        }
    };

    for sel in selectors {
        let mut matched_objects = 0usize;

        // Unprefixed index selectors mean nodes; unprefixed patterns look at
        // every kind of named object.
        let look_at_nodes = matches!(sel.kind, Kind::Any | Kind::Node);
        let look_at_meshes =
            sel.kind == Kind::Mesh || (sel.kind == Kind::Any && sel.matcher.is_pattern());
        let look_at_scenes =
            sel.kind == Kind::Scene || (sel.kind == Kind::Any && sel.matcher.is_pattern());

        if look_at_nodes {
            for node in doc.nodes() {
                if sel.matcher.matches(node.index(), node.name()) {
                    matched_objects += 1;
                    add_node(node.index(), &mut selected);
                }
            }
        }

        if look_at_meshes {
            for mesh in doc.meshes() {
                if sel.matcher.matches(mesh.index(), mesh.name()) {
                    matched_objects += 1;
                    for node in doc.nodes() {
                        if node.mesh().map(|m| m.index()) == Some(mesh.index()) {
                            selected.insert(node.index());
                        }
                    }
                }
            }
        }

        if look_at_scenes {
            for scene in doc.scenes() {
                if sel.matcher.matches(scene.index(), scene.name()) {
                    matched_objects += 1;
                    for root in scene.nodes() {
                        add_node(root.index(), &mut selected);
                    }
                }
            }
        }

        if matched_objects == 0 {
            warn_code!(codes::SELECTOR_NO_MATCH, "selector {} matched no objects", sel.raw);
        }
    }

    selected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::tests::doc;

    // scene "Main": [Root(mesh 0 "Shape") -> ChairA(mesh 0), ChairB(mesh 1 "Other")], node Lone not in scene.
    const MODEL: &str = r#"{
        "asset": {"version": "2.0"},
        "scenes": [{"name": "Main", "nodes": [0]}],
        "meshes": [
            {"name": "Shape", "primitives": [{"attributes": {}}]},
            {"name": "Other", "primitives": [{"attributes": {}}]}
        ],
        "nodes": [
            {"name": "Root", "children": [1, 2]},
            {"name": "ChairA", "mesh": 0},
            {"name": "ChairB", "mesh": 1},
            {"name": "Lone", "mesh": 0}
        ]
    }"#;

    fn run(sels: &[&str], descendants: bool) -> Vec<usize> {
        let d = doc(MODEL);
        let h = Hierarchy::new(&d);
        let parsed: Vec<Selector> = sels.iter().map(|s| parse_selector(s).unwrap()).collect();
        resolve(&d, &h, &parsed, descendants).into_iter().collect()
    }

    #[test]
    fn glob_on_node_names() {
        assert_eq!(run(&["Chair*"], true), vec![1, 2]);
        assert_eq!(run(&["ChairA", "ChairB"], true), vec![1, 2]);
        assert_eq!(run(&["Chair?"], true), vec![1, 2]);
    }

    #[test]
    fn parent_includes_descendants_unless_disabled() {
        assert_eq!(run(&["Root"], true), vec![0, 1, 2]);
        assert_eq!(run(&["Root"], false), vec![0]);
    }

    #[test]
    fn mesh_selector_picks_every_instance() {
        assert_eq!(run(&["mesh:Shape"], true), vec![1, 3]);
        // unprefixed patterns also match mesh names
        assert_eq!(run(&["Shape"], true), vec![1, 3]);
    }

    #[test]
    fn scene_selector_takes_roots_and_descendants() {
        assert_eq!(run(&["scene:Main"], true), vec![0, 1, 2]);
        assert_eq!(run(&["scene:Main"], false), vec![0]);
        assert_eq!(run(&["Main"], true), vec![0, 1, 2]);
    }

    #[test]
    fn indices_are_nodes_unless_prefixed() {
        assert_eq!(run(&["#3"], true), vec![3]);
        assert_eq!(run(&["mesh:#1"], true), vec![2]);
        assert_eq!(run(&["scene:#0"], false), vec![0]);
    }

    #[test]
    fn overlapping_selectors_do_not_duplicate() {
        assert_eq!(run(&["Root", "ChairA", "#1"], true), vec![0, 1, 2]);
    }

    #[test]
    fn no_match_is_empty_not_an_error() {
        assert!(run(&["Nothing*", "#99"], true).is_empty());
    }

    #[test]
    fn quoted_names_and_bad_patterns() {
        assert!(parse_selector("node:\"My Chair*\"").is_ok());
        let e = parse_selector("[").unwrap_err();
        assert_eq!(crate::diag::code_of(&e), codes::SELECTOR_INVALID_PATTERN);
    }
}
