//! The glTF node hierarchy: world matrices and descendant traversal.

use crate::diag::codes;
use crate::xform::{self, Mat4};
use anyhow::Result;
use gltf::Document;
use std::collections::BTreeSet;

pub struct Hierarchy {
    parent: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    local: Vec<Mat4>,
}

impl Hierarchy {
    pub fn new(doc: &Document) -> Hierarchy {
        let count = doc.nodes().count();
        let mut parent = vec![None; count];
        let mut children = vec![Vec::new(); count];
        let mut local = vec![xform::IDENTITY; count];

        for node in doc.nodes() {
            let i = node.index();
            local[i] = node.transform().matrix();
            for child in node.children() {
                children[i].push(child.index());
                parent[child.index()] = Some(i);
            }
        }

        Hierarchy {
            parent,
            children,
            local,
        }
    }

    /// The accumulated world matrix of `node` (root-to-node product of local
    /// matrices). Fails if the parent chain loops (only possible in files
    /// that skipped validation, e.g. with `--lenient`).
    pub fn world(&self, node: usize) -> Result<Mat4> {
        let mut chain = vec![node];
        let mut current = node;
        while let Some(p) = self.parent[current] {
            chain.push(p);
            current = p;
            if chain.len() > self.parent.len() {
                bail_code!(
                    codes::NODE_HIERARCHY_CYCLE,
                    "node #{node} is part of a cycle in the node hierarchy"
                );
            }
        }
        let mut m = xform::IDENTITY;
        for &n in chain.iter().rev() {
            m = xform::mul(&m, &self.local[n]);
        }
        Ok(m)
    }

    /// Insert `node` and all of its descendants into `out`.
    pub fn insert_with_descendants(&self, node: usize, out: &mut BTreeSet<usize>) {
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            if out.insert(n) {
                stack.extend(self.children[n].iter().copied());
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn doc(json: &str) -> Document {
        gltf::Gltf::from_slice_without_validation(json.as_bytes())
            .unwrap()
            .document
    }

    // 0 (translate x+10) -> 1 (scale 2) -> 2 ; 3 is a separate root.
    const TREE: &str = r#"{
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0, 3]}],
        "nodes": [
            {"name": "a", "translation": [10, 0, 0], "children": [1]},
            {"name": "b", "scale": [2, 2, 2], "children": [2]},
            {"name": "c", "translation": [1, 0, 0]},
            {"name": "d"}
        ]
    }"#;

    #[test]
    fn world_matrix_accumulates_parents() {
        let h = Hierarchy::new(&doc(TREE));
        let w = h.world(2).unwrap();
        // c's origin: translate(10) * scale(2) * translate(1) -> x = 12
        assert_eq!(w[3][0], 12.0);
        assert_eq!(h.world(3).unwrap(), xform::IDENTITY);
    }

    #[test]
    fn descendants_include_the_node_itself() {
        let h = Hierarchy::new(&doc(TREE));
        let mut out = BTreeSet::new();
        h.insert_with_descendants(0, &mut out);
        assert_eq!(out.into_iter().collect::<Vec<_>>(), vec![0, 1, 2]);
    }

    #[test]
    fn cycles_are_reported_not_looped_forever() {
        let json = r#"{
            "asset": {"version": "2.0"},
            "nodes": [{"children": [1]}, {"children": [0]}]
        }"#;
        let d = gltf::Gltf::from_slice_without_validation(json.as_bytes())
            .unwrap()
            .document;
        let h = Hierarchy::new(&d);
        let e = h.world(0).unwrap_err();
        assert_eq!(crate::diag::code_of(&e), codes::NODE_HIERARCHY_CYCLE);
        // and descendant traversal terminates
        let mut out = BTreeSet::new();
        h.insert_with_descendants(0, &mut out);
        assert_eq!(out.len(), 2);
    }
}
