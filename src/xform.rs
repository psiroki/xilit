//! 4x4 matrix helpers and the vertex transforms used to bake a node's world
//! matrix into vertex data (spec 2.0 section 4). All math is `f32`.
//!
//! Matrices are column-major, exactly like glTF and `gltf::Node::transform()`:
//! `m[column][row]`.

pub type Mat4 = [[f32; 4]; 4];

pub const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `a * b` (apply `b` first, then `a`).
pub fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut r = [[0.0f32; 4]; 4];
    for (c, column) in r.iter_mut().enumerate() {
        for (row, cell) in column.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[k][row] * b[c][k]).sum();
        }
    }
    r
}

/// Determinant of the linear (upper-left 3x3) part. Negative means the
/// transform mirrors geometry (reflection / negative scale).
pub fn linear_det(m: &Mat4) -> f32 {
    let a = |row: usize, col: usize| m[col][row];
    a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
        - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0))
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 0.0 && len.is_finite() {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        v
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// A world matrix prepared for transforming points, normals and tangents.
pub struct Xform {
    m: Mat4,
    /// Cofactor matrix of the linear part, `cof[row][col]`. For a matrix
    /// with positive determinant this is the inverse transpose up to a
    /// positive scale, which is irrelevant once the normal is re-normalized.
    cof: [[f32; 3]; 3],
}

impl Xform {
    pub fn new(m: Mat4) -> Xform {
        let a = |row: usize, col: usize| m[col][row];
        let cof = [
            [
                a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1),
                -(a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0)),
                a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0),
            ],
            [
                -(a(0, 1) * a(2, 2) - a(0, 2) * a(2, 1)),
                a(0, 0) * a(2, 2) - a(0, 2) * a(2, 0),
                -(a(0, 0) * a(2, 1) - a(0, 1) * a(2, 0)),
            ],
            [
                a(0, 1) * a(1, 2) - a(0, 2) * a(1, 1),
                -(a(0, 0) * a(1, 2) - a(0, 2) * a(1, 0)),
                a(0, 0) * a(1, 1) - a(0, 1) * a(1, 0),
            ],
        ];
        Xform { m, cof }
    }

    fn linear(&self, v: [f32; 3]) -> [f32; 3] {
        let m = &self.m;
        [
            m[0][0] * v[0] + m[1][0] * v[1] + m[2][0] * v[2],
            m[0][1] * v[0] + m[1][1] * v[1] + m[2][1] * v[2],
            m[0][2] * v[0] + m[1][2] * v[1] + m[2][2] * v[2],
        ]
    }

    /// Transform a position (includes translation).
    pub fn point(&self, p: [f32; 3]) -> [f32; 3] {
        let l = self.linear(p);
        [l[0] + self.m[3][0], l[1] + self.m[3][1], l[2] + self.m[3][2]]
    }

    /// Transform a normal by the inverse transpose of the linear part, then
    /// normalize.
    pub fn normal(&self, n: [f32; 3]) -> [f32; 3] {
        let c = &self.cof;
        normalize([
            c[0][0] * n[0] + c[0][1] * n[1] + c[0][2] * n[2],
            c[1][0] * n[0] + c[1][1] * n[1] + c[1][2] * n[2],
            c[2][0] * n[0] + c[2][1] * n[1] + c[2][2] * n[2],
        ])
    }

    /// Transform a tangent by the linear part, make it orthogonal to
    /// `world_normal` (the already-transformed, normalized normal) and
    /// normalize.
    pub fn tangent(&self, t: [f32; 3], world_normal: [f32; 3]) -> [f32; 3] {
        let l = self.linear(t);
        let d = dot(world_normal, l);
        normalize([
            l[0] - world_normal[0] * d,
            l[1] - world_normal[1] * d,
            l[2] - world_normal[2] * d,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translation(x: f32, y: f32, z: f32) -> Mat4 {
        let mut m = IDENTITY;
        m[3] = [x, y, z, 1.0];
        m
    }

    fn scale(x: f32, y: f32, z: f32) -> Mat4 {
        [
            [x, 0.0, 0.0, 0.0],
            [0.0, y, 0.0, 0.0],
            [0.0, 0.0, z, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5)
    }

    #[test]
    fn mul_applies_right_operand_first() {
        // scale by 2, then translate by (1,0,0)
        let m = mul(&translation(1.0, 0.0, 0.0), &scale(2.0, 2.0, 2.0));
        assert!(close(Xform::new(m).point([1.0, 1.0, 1.0]), [3.0, 2.0, 2.0]));
    }

    #[test]
    fn point_gets_translation_normal_does_not() {
        let x = Xform::new(translation(5.0, 6.0, 7.0));
        assert!(close(x.point([1.0, 2.0, 3.0]), [6.0, 8.0, 10.0]));
        assert!(close(x.normal([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn normal_uses_inverse_transpose() {
        // Stretching x by 2 must tilt a 45-degree normal toward y.
        let x = Xform::new(scale(2.0, 1.0, 1.0));
        let n = x.normal([1.0, 1.0, 0.0]);
        let len = (0.25f32 + 1.0).sqrt();
        assert!(close(n, [0.5 / len, 1.0 / len, 0.0]));
    }

    #[test]
    fn normal_is_normalized_after_uniform_scale() {
        let x = Xform::new(scale(3.0, 3.0, 3.0));
        assert!(close(x.normal([0.0, 2.0, 0.0]), [0.0, 1.0, 0.0]));
    }

    #[test]
    fn tangent_is_orthogonal_to_normal_and_unit_length() {
        let x = Xform::new(scale(2.0, 1.0, 1.0));
        let n = x.normal([1.0, 1.0, 0.0]);
        let t = x.tangent([1.0, -1.0, 0.0], n);
        assert!(dot(n, t).abs() < 1e-5);
        assert!((dot(t, t) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn determinant_sign() {
        assert!(linear_det(&scale(1.0, 2.0, 3.0)) > 0.0);
        assert!(linear_det(&scale(-1.0, 1.0, 1.0)) < 0.0);
        // two reflections cancel out
        assert!(linear_det(&scale(-1.0, -1.0, 1.0)) > 0.0);
        assert_eq!(linear_det(&scale(0.0, 1.0, 1.0)), 0.0);
    }
}
