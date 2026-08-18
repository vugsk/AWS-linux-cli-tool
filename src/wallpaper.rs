use std::{path::{Path, PathBuf}, collections::HashSet};
use std::process::Command;
use image::ImageReader;
use kmeans_color_gpu::RGBA8;
use rand::prelude::IndexedRandom;

use crate::color::{rgba8_to_hsl, hue_distance};
use crate::config::Transition;

// Граница яркости (L в HSL, 0–100): ниже — «тёмный» фон, выше — «светлый»
pub const DARK_LIGHTNESS: f32 = 42.0;

pub fn local_hour() -> u32 {
	Command::new("date")
		.arg("+%H")
		.output()
		.ok()
		.and_then(|o| String::from_utf8(o.stdout).ok())
		.and_then(|s| s.trim().parse().ok())
		.unwrap_or(12)
}

// Вечер (17–23) и ночь (0–5) — предпочитаем тёмные обои
pub fn prefer_dark(hour: u32) -> bool {
	matches!(hour, 0..=5 | 17..=23)
}

// Целевая светлость по времени суток: косинус-кривая с пиком яркости в 13:00
// (самая светлая) и минимумом в 01:00 (самая тёмная). Папка выбирается по
// диапазону вокруг этого значения.
pub fn target_lightness(hour: u32) -> f32 {
	let angle = (hour as f32 - 13.0) / 24.0 * std::f32::consts::TAU;
	50.0 + 45.0 * angle.cos() // диапазон ~5..95
}

// Усредняем H и L покоорганно по каждому пикселю для точной оценки яркости фона
pub fn average_hsl(path: &Path) -> Option<(f32, f32)> {
	let img = ImageReader::open(path).ok()?.decode().ok()?
		.thumbnail(64, 64)
		.to_rgba8();

	let count = (img.width() * img.height()) as u64;
	if count == 0 {
		return None;
	}

	let (mut h_sum, mut l_sum) = (0.0f64, 0.0f64);
	for px in img.pixels() {
		let hsl = rgba8_to_hsl(&RGBA8 { r: px[0], g: px[1], b: px[2], a: 255 });
		h_sum += hsl.h as f64;
		l_sum += hsl.l as f64;
	}

	Some((
		(h_sum / count as f64) as f32,
		(l_sum / count as f64) as f32,
	))
}

// Выбирает обои из пула с учётом времени суток (тёмные/светлые) и близости по оттенку И яркости
pub fn pick_similar(
	images: &[PathBuf],
	hue_tolerance: f32,
	lightness_tolerance: f32,
	exclude: &HashSet<PathBuf>,
) -> Option<PathBuf> {
	let want_dark = prefer_dark(local_hour());
	let mut rng = rand::rng();

	let candidates: Vec<(PathBuf, f32, f32)> = images.iter()
		.filter(|p| !exclude.contains(*p))
		.filter_map(|p| average_hsl(p).map(|(h, l)| (p.clone(), h, l)))
		.collect();

	if candidates.is_empty() {
		return None;
	}

	// Индексы, соответствующие текущему времени суток (тёмные или светлые)
	let time_pool: Vec<usize> = candidates.iter().enumerate()
		.filter_map(|(i, (_, _, l))| {
			let fits = if want_dark { *l < DARK_LIGHTNESS } else { *l >= DARK_LIGHTNESS };
			fits.then_some(i)
		})
		.collect();

	// Если подходящих по времени нет — берём из всех
	let pool: Vec<usize> = if time_pool.is_empty() {
		(0..candidates.len()).collect()
	} else {
		time_pool
	};

	let &anchor_idx = pool.choose(&mut rng)?;
	let anchor_h = candidates[anchor_idx].1;
	let anchor_l = candidates[anchor_idx].2;

	// Отбираем близкие и по оттенку, и по яркости
	let similar: Vec<&PathBuf> = candidates.iter()
		.filter(|(_, h, l)| {
			hue_distance(*h, anchor_h) <= hue_tolerance
				&& (l - anchor_l).abs() <= lightness_tolerance
		})
		.map(|(p, _, _)| p)
		.collect();

	similar.choose(&mut rng).map(|p| (*p).clone())
}

// Выбирает наиболее близкие к anchor обои (fallback когда pick_matching ничего не нашёл)
pub fn pick_closest(
	images: &[PathBuf],
	anchor_h: f32,
	anchor_l: f32,
	exclude: &HashSet<PathBuf>,
) -> Option<PathBuf> {
	let mut rng = rand::rng();
	let mut scored: Vec<(PathBuf, f32)> = images.iter()
		.filter(|p| !exclude.contains(*p))
		.filter_map(|p| {
			average_hsl(p).map(|(h, l)| {
				// Яркость весит вдвое больше оттенка: разница фона заметнее
				let d = hue_distance(h, anchor_h) * 0.5 + (l - anchor_l).abs() * 1.0;
				(p.clone(), d)
			})
		})
		.collect();

	if scored.is_empty() { return None; }

	scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
	// Небольшой выбор из лучших чтобы не всегда брать одно и то же
	let top = ((scored.len() / 5) + 1).min(3);
	scored[..top].choose(&mut rng).map(|(p, _)| p.clone())
}

// Подбирает обои, подходящие к уже выбранному anchor (для второго и последующих экранов)
pub fn pick_matching(
	images: &[PathBuf],
	anchor_h: f32,
	anchor_l: f32,
	hue_tolerance: f32,
	lightness_tolerance: f32,
	exclude: &HashSet<PathBuf>,
) -> Option<PathBuf> {
	let mut rng = rand::rng();
	let matched: Vec<PathBuf> = images.iter()
		.filter(|p| !exclude.contains(*p))
		.filter(|p| {
			average_hsl(p).map(|(h, l)| {
				hue_distance(h, anchor_h) <= hue_tolerance
					&& (l - anchor_l).abs() <= lightness_tolerance
			}).unwrap_or(false)
		})
		.cloned()
		.collect();
	matched.choose(&mut rng).cloned()
}

pub fn set_wallpaper(
	tool: &str,
	path: &PathBuf,
	display_name: &str,
	mode: &str,
	transition: Option<&Transition>,
) -> Result<(), Box<dyn std::error::Error>> {
	match tool {
		"awww" => {
			let resize = match mode {
				"fit" | "contain" => "fit",
				"stretch" => "no",
				_ => "crop",
			};
			let mut cmd = Command::new("awww");
			cmd.arg("img").arg(path);
			if !display_name.is_empty() {
				cmd.arg("--outputs").arg(display_name);
			}
			cmd.arg("--resize").arg(resize);
			if let Some(t) = transition {
				cmd.arg("--transition-type").arg(&t.kind)
					.arg("--transition-duration").arg(t.duration.to_string())
					.arg("--transition-fps").arg(t.fps.to_string());
			}
			run_cmd(&mut cmd, tool)?;
		}
		"swaybg" => {
			let bg_mode = match mode {
				"fit" | "contain" => "fit",
				"tile" => "tile",
				"center" => "center",
				_ => "fill",
			};
			let mut cmd = Command::new("swaybg");
			if !display_name.is_empty() {
				cmd.arg("-o").arg(display_name);
			}
			cmd.arg("-i").arg(path).arg("-m").arg(bg_mode);
			run_cmd(&mut cmd, tool)?;
		}
		"feh" => {
			let feh_mode = match mode {
				"fit" | "contain" => "--bg-max",
				"tile" => "--bg-tile",
				"center" => "--bg-center",
				"stretch" => "--bg-scale",
				_ => "--bg-fill",
			};
			run_cmd(Command::new("feh").arg(feh_mode).arg(path), tool)?;
		}
		other => eprintln!("Неизвестный инструмент: '{other}'. Поддерживаются: awww, swaybg, feh"),
	}
	Ok(())
}

pub fn run_cmd(cmd: &mut Command, tool: &str) -> Result<(), Box<dyn std::error::Error>> {
	match cmd.output() {
		Ok(out) if !out.status.success() =>
			eprintln!("{tool}: {}", String::from_utf8_lossy(&out.stderr).trim()),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound =>
			eprintln!("Инструмент не найден: '{tool}'. Убедитесь, что он установлен."),
		Err(e) => return Err(Box::new(e)),
		_ => {}
	}
	Ok(())
}

pub fn collect_images(dir: &str) -> Result<Vec<PathBuf>, String> {
	match std::fs::read_dir(dir) {
		Ok(entries) => Ok(entries
			.filter_map(|e| e.ok())
			.map(|e| e.path())
			.filter(|p| matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("png" | "jpg" | "jpeg" | "webp")
            ))
			.collect()),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound =>
			Err(format!("Директория не найдена: {dir}")),
		Err(e) =>
			Err(format!("Ошибка чтения директории '{dir}': {e}")),
	}
}
