use super::atlas;
use anyhow::{Context, Result, ensure};
use image::{Rgba, RgbaImage};
use std::io::{BufRead, Seek, SeekFrom};

const LIMIT: u64 = 128 * 1024 * 1024;

fn samples(extent: u32, count: u32, size: u32) -> Vec<u32> {
    (0..count)
        .flat_map(|face| {
            let start = ((u64::from(face) * u64::from(extent) + u64::from(count / 2))
                / u64::from(count)) as u32;
            let end = ((u64::from(face + 1) * u64::from(extent) + u64::from(count / 2))
                / u64::from(count)) as u32;
            (0..size).map(move |offset| {
                start
                    + ((u64::from(offset * 2 + 1) * u64::from(end - start)) / u64::from(size * 2))
                        as u32
            })
        })
        .collect()
}

fn shrink(image: RgbaImage) -> Result<RgbaImage> {
    let size = atlas::dimensions(&image)?.min(256);
    if image.dimensions() == (size * 3, size * 2) {
        return Ok(image);
    }
    let xs = samples(image.width(), 3, size);
    let ys = samples(image.height(), 2, size);
    Ok(RgbaImage::from_fn(size * 3, size * 2, |x, y| {
        *image.get_pixel(xs[x as usize], ys[y as usize])
    }))
}

pub(super) fn read(mut source: impl BufRead + Seek) -> Result<RgbaImage> {
    let mut magic = [0; 8];
    let count = source.read(&mut magic)?;
    source.seek(SeekFrom::Start(0))?;
    if count == 8 && magic == *b"\x89PNG\r\n\x1a\n" {
        let mut decoder = png::Decoder::new(&mut source);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info()?;
        let width = reader.info().width;
        let height = reader.info().height;
        ensure!(
            u64::from(width) * u64::from(height) <= LIMIT,
            "天空预览像素数量过大"
        );
        let size = atlas::face_size(width, height)?.min(256);
        if !reader.info().interlaced && reader.info().animation_control.is_none() {
            let color = reader.output_color_type().0;
            let channels = color.samples();
            let xs = samples(width, 3, size);
            let ys = samples(height, 2, size);
            let mut image = RgbaImage::new(size * 3, size * 2);
            let mut target_y = 0;
            for row_y in 0..height {
                let row = reader.next_row()?.context("天空 PNG 行数据不完整")?;
                while target_y < ys.len() && ys[target_y] == row_y {
                    for (target_x, &source_x) in xs.iter().enumerate() {
                        let offset = source_x as usize * channels;
                        let pixel = &row.data()[offset..offset + channels];
                        let rgba = match color {
                            png::ColorType::Rgba => [pixel[0], pixel[1], pixel[2], pixel[3]],
                            png::ColorType::Rgb => [pixel[0], pixel[1], pixel[2], 255],
                            png::ColorType::Grayscale => [pixel[0], pixel[0], pixel[0], 255],
                            png::ColorType::GrayscaleAlpha => {
                                [pixel[0], pixel[0], pixel[0], pixel[1]]
                            }
                            png::ColorType::Indexed => anyhow::bail!("天空 PNG 调色板未展开"),
                        };
                        image.put_pixel(target_x as u32, target_y as u32, Rgba(rgba));
                    }
                    target_y += 1;
                }
            }
            reader.finish()?;
            ensure!(target_y == ys.len(), "天空预览行数不足");
            return Ok(image);
        }
        drop(reader);
        source.seek(SeekFrom::Start(0))?;
    }
    shrink(
        image::ImageReader::new(source)
            .with_guessed_format()?
            .decode()?
            .to_rgba8(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn streamed_png_preserves_six_face_order_and_alpha() {
        let colors = [
            [220, 10, 30, 255],
            [20, 180, 40, 128],
            [40, 60, 230, 64],
            [220, 180, 40, 0],
            [180, 40, 180, 255],
            [40, 180, 190, 200],
        ];
        let image = RgbaImage::from_fn(1536, 1024, |x, y| {
            Rgba(colors[((y / 512) * 3 + x / 512) as usize])
        });
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let result = read(Cursor::new(bytes.into_inner())).unwrap();
        assert_eq!(result.dimensions(), (768, 512));
        for (index, color) in colors.iter().enumerate() {
            assert_eq!(
                result
                    .get_pixel(index as u32 % 3 * 256 + 128, index as u32 / 3 * 256 + 128)
                    .0,
                *color
            );
        }
    }

    #[test]
    fn indexed_png_and_non_integer_face_sizes_are_supported() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2560, 1706);
            encoder.set_color(png::ColorType::Indexed);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_palette(vec![30, 100, 220]);
            encoder.set_trns(vec![128]);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![0; 2560 * 1706])
                .unwrap();
        }
        let result = read(Cursor::new(bytes)).unwrap();
        assert_eq!(result.dimensions(), (768, 512));
        assert!(result.pixels().all(|pixel| pixel.0 == [30, 100, 220, 128]));
    }

    #[test]
    fn png_named_jpeg_data_keeps_actual_format_detection() {
        let image = image::RgbImage::from_pixel(1536, 1024, image::Rgb([60, 120, 200]));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, image::ImageFormat::Jpeg)
            .unwrap();
        let result = read(Cursor::new(bytes.into_inner())).unwrap();
        assert_eq!(result.dimensions(), (768, 512));
        assert!(
            result
                .pixels()
                .all(|pixel| pixel[2] > pixel[1] && pixel[1] > pixel[0] && pixel[3] == 255)
        );
    }
}
