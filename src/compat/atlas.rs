use anyhow::{Result, ensure};
use image::{RgbaImage, imageops};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasLayout {
    Optifine,
    FabricSkyboxes,
    Nuit,
}

type Vector = [i32; 3];
type Matrix = [[i32; 3]; 3];
const I: Matrix = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
const X: Matrix = [[1, 0, 0], [0, 0, -1], [0, 1, 0]];
const Z: Matrix = [[0, -1, 0], [1, 0, 0], [0, 0, 1]];
const Y: Matrix = [[0, 0, 1], [0, 1, 0], [-1, 0, 0]];

fn mul(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}
fn pow(a: Matrix, n: usize) -> Matrix {
    (0..n).fold(I, |out, _| mul(out, a))
}
fn apply(a: Matrix, v: Vector) -> Vector {
    std::array::from_fn(|i| (0..3).map(|j| a[i][j] * v[j]).sum())
}
fn dot(a: Vector, b: Vector) -> i32 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn faces(layout: AtlasLayout, overworld: bool) -> [(usize, Matrix); 6] {
    match layout {
        AtlasLayout::Optifine => {
            let base = mul(if overworld { pow(Y, 3) } else { I }, mul(X, pow(Z, 3)));
            [
                (4, base),
                (1, mul(base, X)),
                (0, mul(base, pow(X, 3))),
                (5, mul(base, Z)),
                (2, mul(base, pow(Z, 2))),
                (3, mul(base, pow(Z, 3))),
            ]
        }
        AtlasLayout::FabricSkyboxes | AtlasLayout::Nuit => {
            let tiles = if layout == AtlasLayout::FabricSkyboxes {
                [4, 2, 5, 3, 1, 0]
            } else {
                [0, 4, 2, 1, 5, 3]
            };
            let matrices = [
                I,
                X,
                mul(pow(X, 3), pow(Y, 2)),
                pow(X, 2),
                mul(Z, pow(Y, 3)),
                mul(pow(Z, 3), Y),
            ];
            std::array::from_fn(|index| (tiles[index], matrices[index]))
        }
    }
}

pub fn dimensions(image: &RgbaImage) -> Result<u32> {
    ensure!(
        image.width().is_multiple_of(3)
            && image.height().is_multiple_of(2)
            && image.width() / 3 == image.height() / 2,
        "天空图需要 3×2 方形六面布局，实际为 {}×{}",
        image.width(),
        image.height()
    );
    let size = image.width() / 3;
    ensure!(size > 0, "天空图不能为空");
    Ok(size)
}

pub fn remap(
    image: &RgbaImage,
    source: AtlasLayout,
    target: AtlasLayout,
    overworld: bool,
) -> Result<RgbaImage> {
    let size = dimensions(image)?;
    if source == target {
        return Ok(image.clone());
    }
    let mut out = RgbaImage::new(image.width(), image.height());
    for (target_tile, target_matrix) in faces(target, overworld) {
        let normal = apply(target_matrix, [0, -1, 0]);
        let (source_tile, source_matrix) = faces(source, overworld)
            .into_iter()
            .find(|(_, matrix)| apply(*matrix, [0, -1, 0]) == normal)
            .unwrap();
        let target_u = apply(target_matrix, [1, 0, 0]);
        let target_v = apply(target_matrix, [0, 0, 1]);
        let source_u = apply(source_matrix, [1, 0, 0]);
        let source_v = apply(source_matrix, [0, 0, 1]);
        for y in 0..size {
            for x in 0..size {
                let u = 2 * x as i32 + 1 - size as i32;
                let v = 2 * y as i32 + 1 - size as i32;
                let source_x =
                    (dot(target_u, source_u) * u + dot(target_v, source_u) * v + size as i32 - 1)
                        / 2;
                let source_y =
                    (dot(target_u, source_v) * u + dot(target_v, source_v) * v + size as i32 - 1)
                        / 2;
                let pixel = image.get_pixel(
                    (source_tile % 3) as u32 * size + source_x as u32,
                    (source_tile / 3) as u32 * size + source_y as u32,
                );
                out.put_pixel(
                    (target_tile % 3) as u32 * size + x,
                    (target_tile / 3) as u32 * size + y,
                    *pixel,
                );
            }
        }
    }
    Ok(out)
}

pub fn faces_preview(image: &RgbaImage) -> Result<Vec<RgbaImage>> {
    let size = dimensions(image)?;
    let canonical = remap(image, AtlasLayout::Optifine, AtlasLayout::Nuit, true)?;
    Ok([4, 2, 5, 3, 1, 0]
        .into_iter()
        .map(|tile| {
            imageops::crop_imm(&canonical, tile % 3 * size, tile / 3 * size, size, size).to_image()
        })
        .collect())
}
