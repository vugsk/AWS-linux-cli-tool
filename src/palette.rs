use std::{fs, path::PathBuf};
use image::ImageReader;
use kmeans_color_gpu::{Algorithm, ImageProcessor, RGBA8};
use kmeans_color_gpu::image::Image;

use crate::color::rgba8_to_hex;

pub fn extract_palette(path: &PathBuf, num_colors: u32) -> Result<Vec<RGBA8>, Box<dyn std::error::Error>> {
	pollster::block_on(async {
		// Ограничиваем размер до 1024 px: текстура WGPU не может быть больше
		// 8192 по стороне, а доминирующие цвета сохраняются и на уменьшенной копии.
		let img_buffer = ImageReader::open(path)?.decode()?.thumbnail(1024, 1024).to_rgba8();
		let dimensions = img_buffer.dimensions();
		let img = Image::new(dimensions, bytemuck::cast_slice(img_buffer.as_raw()));
		let proc = ImageProcessor::new().await?;
		let pal = proc.palette(num_colors, &img, Algorithm::Kmeans).await?;
		Ok(pal)
	})
}

pub fn write_palette(
	output: &PathBuf,
	mapping: &[(String, RGBA8)],
	dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
	if dry_run {
		for (role, color) in mapping {
			println!("COLOR_{:<12} {}", role, rgba8_to_hex(color));
		}
		return Ok(());
	}

	let file_existed = output.exists();

	if let Some(parent) = output.parent() {
		if !parent.exists() {
			fs::create_dir_all(parent)?;
			println!("Создана папка: {}", parent.display());
		}
	}

	let content = mapping.iter()
		.map(|(role, color)| format!("set -gx COLOR_{} \"{}\"", role, rgba8_to_hex(color)))
		.collect::<Vec<_>>()
		.join("\n") + "\n";

	fs::write(output, content)?;

	if file_existed {
		println!("Палитра обновлена: {}", output.display());
	} else {
		println!("Палитра создана: {}", output.display());
	}

	Ok(())
}
