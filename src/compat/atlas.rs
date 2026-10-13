use anyhow::{Result, ensure};
use image::{RgbaImage, imageops};
use std::borrow::Cow;

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

pub fn face_size(width: u32, height: u32) -> Result<u32> {
    let horizontal = u64::from(width) * 2;
    let vertical = u64::from(height) * 3;
    ensure!(
        width >= 3 && height >= 2 && horizontal.abs_diff(vertical) * 10 <= vertical,
        "天空图未识别为 3×2 六面布局，实际为 {width}×{height}"
    );
    let size = width.div_ceil(3).max(height.div_ceil(2));
    ensure!(size <= u32::MAX / 3, "天空单面尺寸无效");
    Ok(size)
}

pub fn dimensions(image: &RgbaImage) -> Result<u32> {
    face_size(image.width(), image.height())
}

pub fn resize_faces(image: &RgbaImage, size: u32) -> Result<RgbaImage> {
    dimensions(image)?;
    ensure!(size > 0 && size <= u32::MAX / 3, "天空单面尺寸无效");
    if image.dimensions() == (3 * size, 2 * size) {
        return Ok(image.clone());
    }
    let mut output = RgbaImage::new(3 * size, 2 * size);
    let boundary = |part: u32, extent: u32, count: u32| {
        ((u64::from(part) * u64::from(extent) + u64::from(count / 2)) / u64::from(count)) as u32
    };
    for row in 0..2 {
        for column in 0..3 {
            let left = boundary(column, image.width(), 3);
            let right = boundary(column + 1, image.width(), 3);
            let top = boundary(row, image.height(), 2);
            let bottom = boundary(row + 1, image.height(), 2);
            let face = imageops::crop_imm(image, left, top, right - left, bottom - top).to_image();
            let face = imageops::resize(&face, size, size, imageops::FilterType::Triangle);
            imageops::replace(
                &mut output,
                &face,
                i64::from(column * size),
                i64::from(row * size),
            );
        }
    }
    Ok(output)
}

pub fn remap(
    image: &RgbaImage,
    source: AtlasLayout,
    target: AtlasLayout,
    overworld: bool,
) -> Result<RgbaImage> {
    let size = dimensions(image)?;
    let image = if image.dimensions() == (3 * size, 2 * size) {
        Cow::Borrowed(image)
    } else {
        Cow::Owned(resize_faces(image, size)?)
    };
    if source == target {
        return Ok(image.into_owned());
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
    let canonical = remap(image, AtlasLayout::Optifine, AtlasLayout::Nuit, true)?;
    let size = canonical.width() / 3;
    Ok([4, 2, 5, 3, 1, 0]
        .into_iter()
        .map(|tile| {
            imageops::crop_imm(&canonical, tile % 3 * size, tile / 3 * size, size, size).to_image()
        })
        .collect())
}
