pub mod cache;
pub mod cli;
pub mod cluster;
pub mod color;
pub mod config;
pub mod generation;
pub mod palette;
pub mod sort;
pub mod state;
pub mod wallpaper;

use std::{collections::HashSet, path::PathBuf};

use kmeans_color_gpu::RGBA8;
use rand::prelude::IndexedRandom;

use crate::color::{map_to_theme, palette_similarity};
use crate::config::Config;
use crate::generation::generate_and_run;
use crate::palette::{extract_palette, write_palette};
use crate::state::{expand_path, load_config, load_used, save_used, state_path};
use crate::wallpaper::{
	average_hsl, collect_images, pick_closest, pick_matching, pick_similar, set_wallpaper,
};

// Выбранные обои-якорь и их палитра (если она уже есть в кэше)
type Anchor = (Option<PathBuf>, Option<Vec<RGBA8>>);

fn main() -> Result<(), Box<dyn std::error::Error>> {
	// Информационные/служебные подкоманды (palette, info, list, folders,
	// new-generator, help) не запускают основной пайплайн.
	if cli::run()? {
		return Ok(());
	}

	let config = load_config()?;

	// Режим хранилища (корзины по светлости + подбор по палитрам) включается,
	// если задан wallpaper.store; иначе работает прежний выбор из dir/path.
	let (first_wallpaper, precomputed) = match config.wallpaper.store.clone() {
		Some(store) => run_store(&config, &store)?,
		None => (run_legacy(&config)?, None),
	};

	if let Some(wp) = first_wallpaper {
		// В режиме хранилища палитра якоря уже посчитана и лежит в кэше —
		// не запускаем GPU повторно. В легаси-режиме извлекаем как раньше.
		let palette = match precomputed {
			Some(p) => Some(p),
			None => match extract_palette(&wp, config.palette.color) {
				Ok(p) => Some(p),
				Err(e) => {
					eprintln!("Ошибка извлечения палитры: {e}");
					None
				}
			},
		};
		if let Some(palette) = palette {
			apply_theme(&config, &palette);
		}
	}

	Ok(())
}

// Применяет палитру к теме: маппинг ролей, запись файла, генерация конфигов.
fn apply_theme(config: &Config, palette: &[RGBA8]) {
	let mapping = map_to_theme(palette, config.palette.sat_threshold as f32, config.palette.min_contrast);
	let palette_path = expand_path(&config.palette.output);
	match write_palette(&palette_path, &mapping, config.behavior.dry_run) {
		Err(e) => eprintln!("Ошибка записи палитры: {e}"),
		Ok(()) if config.behavior.generation && !config.behavior.dry_run => {
			if let Err(e) = generate_and_run(
				&config.behavior.generation_conf,
				&config.behavior.scripts_dir,
				&palette_path.to_string_lossy(),
			) {
				eprintln!("Ошибка генерации: {e}");
			}
		}
		Ok(()) => {}
	}
}

// Режим хранилища: импорт/кластеризация при необходимости, выбор папки по
// диапазону светлости (время суток) и подбор обоев по экранам со сходством
// палитр. Возвращает первые выбранные обои и их палитру (для применения темы).
fn run_store(
	config: &Config,
	store_str: &str,
) -> Result<Anchor, Box<dyn std::error::Error>> {
	let store = expand_path(store_str);

	// 1. Сортировка: при появлении новых обоев в dirs/ или отсутствии folders.json
	let mut cache = cache::load();
	if let Err(e) = sort::run_sort(&store, config, &mut cache) {
		eprintln!("Сортировка: {e}");
	}
	cache::save(&cache)?;

	// 2. Папка по времени суток: целевая светлость → диапазон [L−W, L+W]
	let folders = sort::load_folders(&store);
	if folders.is_empty() {
		eprintln!("В хранилище нет обоев: {}", store.display());
		return Ok((None, None));
	}
	let target = wallpaper::target_lightness(wallpaper::local_hour());
	let window = config.wallpaper.lightness_window;
	let mut rng = rand::rng();
	let in_window: Vec<&sort::FolderMeta> = folders
		.iter()
		.filter(|f| (f.avg_lightness - target).abs() <= window)
		.collect();
	let folder = match in_window.choose(&mut rng) {
		Some(f) => *f,
		// Если в диапазон не попала ни одна папка — берём ближайшую по светлости
		None => folders
			.iter()
			.min_by(|a, b| {
				(a.avg_lightness - target)
					.abs()
					.partial_cmp(&(b.avg_lightness - target).abs())
					.unwrap()
			})
			.unwrap(),
	};
	println!(
		"Папка по времени (L≈{:.0}): {} (avg L={:.0}, cohesion={:.2}, {} обоев)",
		target,
		folder.name,
		folder.avg_lightness,
		folder.cohesion,
		folder.members.len()
	);

	let images: Vec<PathBuf> = folder.members.iter().map(PathBuf::from).collect();

	// 3. used: сбрасываем историю, если папка полностью исчерпана
	let state = state_path();
	let mut used: HashSet<PathBuf> = load_used(&state);
	if images.iter().all(|p| used.contains(p)) {
		for p in &images {
			used.remove(p);
		}
	}

	let tool = &config.wallpaper.tool;
	let transition = config.wallpaper.transition.as_ref();

	// Экраны: имена для --outputs и режим масштабирования; либо один безымянный
	let screens: Vec<(String, String)> = if config.display.is_empty() {
		vec![(String::new(), config.wallpaper.mode.clone())]
	} else {
		config
			.display
			.iter()
			.map(|d| {
				let mode = if d.mode.is_empty() {
					config.wallpaper.mode.clone()
				} else {
					d.mode.clone()
				};
				(d.name.clone(), mode)
			})
			.collect()
	};

	let palette_of = |p: &PathBuf| -> Option<Vec<[u8; 3]>> {
		cache.get(&p.to_string_lossy().into_owned()).map(|e| e.palette.clone())
	};

	let mut first: Option<PathBuf> = None;
	let mut first_palette: Option<Vec<RGBA8>> = None;
	let mut anchor: Option<Vec<[u8; 3]>> = None;

	for (name, mode) in &screens {
		let pick = match &anchor {
			// Первый экран (или если у якоря не оказалось палитры) — случайные
			// обои из корзины
			None => {
				let avail: Vec<&PathBuf> =
					images.iter().filter(|p| !used.contains(*p)).collect();
				avail.choose(&mut rng).map(|p| (*p).clone())
			}
			// Последующие экраны — со сходством палитр в [match_min, match_max)
			Some(anchor_pal) => {
				let matched: Vec<&PathBuf> = images
					.iter()
					.filter(|p| !used.contains(*p))
					.filter(|p| {
						palette_of(p)
							.map(|pal| {
								let s = palette_similarity(
									anchor_pal,
									&pal,
									config.palette.match_delta_e,
								);
								s >= config.palette.match_min && s < config.palette.match_max
							})
							.unwrap_or(false)
					})
					.collect();

				if let Some(p) = matched.choose(&mut rng) {
					Some((*p).clone())
				} else {
					// Фолбэк: ближайший по сходству, но не дубль (< match_max)
					let mut scored: Vec<(&PathBuf, f32)> = images
						.iter()
						.filter(|p| !used.contains(*p))
						.filter_map(|p| {
							palette_of(p).map(|pal| {
								(
									p,
									palette_similarity(
										anchor_pal,
										&pal,
										config.palette.match_delta_e,
									),
								)
							})
						})
						.filter(|(_, s)| *s < config.palette.match_max)
						.collect();
					scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
					scored.first().map(|(p, _)| (*p).clone())
				}
			}
		};

		let label = if name.is_empty() { "экран" } else { name.as_str() };
		let Some(path) = pick else {
			eprintln!("[{label}] Не нашлось подходящих обоев в папке {}", folder.name);
			continue;
		};

		set_wallpaper(tool, &path, name, mode, transition)?;
		if name.is_empty() {
			println!("{}", path.display());
		} else {
			println!("{} {}", path.display(), name);
		}

		if first.is_none() {
			let pal = palette_of(&path);
			first_palette = pal.as_ref().map(|v| {
				v.iter()
					.map(|c| RGBA8 { r: c[0], g: c[1], b: c[2], a: 255 })
					.collect()
			});
			anchor = pal;
			first = Some(path.clone());
		}
		used.insert(path);
	}

	save_used(&state, &used)?;
	Ok((first, first_palette))
}

// Прежний режим: выбор из wallpaper.dir / wallpaper.path или per-display dir/path.
fn run_legacy(config: &Config) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
	let tool = &config.wallpaper.tool;
	let mut mode: String = config.wallpaper.mode.clone();
	let transition = config.wallpaper.transition.as_ref();

	let state = state_path();
	let mut used: HashSet<PathBuf> = load_used(&state);

	let mut first_wallpaper: Option<PathBuf> = None;

	if config.display.is_empty() {
		match (&config.wallpaper.dir, &config.wallpaper.path) {
			(Some(dir), _) => match collect_images(dir) {
				Err(msg) => eprintln!("{msg}"),
				Ok(images) => {
					if images.iter().all(|p| used.contains(p)) {
						for p in &images {
							used.remove(p);
						}
					}
					if let Some(path) = pick_similar(&images, 40.0, 20.0, &used) {
						set_wallpaper(tool, &path, "", &mode, transition)?;
						println!("{}", path.display());
						first_wallpaper = Some(path.clone());
						used.insert(path);
					}
				}
			},
			(None, Some(path)) => {
				let p = PathBuf::from(path);
				if !p.exists() {
					eprintln!("Файл не найден: {path}");
				} else {
					set_wallpaper(tool, &p, "", &mode, transition)?;
					println!("{path}");
					first_wallpaper = Some(p);
				}
			}
			(None, None) => eprintln!("Не указан путь или директория в [wallpaper]"),
		}
	} else {
		// Якорный цвет первого выбранного обоя — остальные экраны к нему подстраиваются
		let mut display_anchor: Option<(f32, f32)> = None;

		for display in &config.display {
			if !display.mode.is_empty() {
				mode = display.mode.clone();
			}

			match (&display.dir, &display.path) {
				(Some(dir), _) => {
					let images = match collect_images(dir) {
						Ok(imgs) => imgs,
						Err(msg) => {
							eprintln!("[{}] {msg}", display.name);
							continue;
						}
					};

					if images.iter().all(|p| used.contains(p)) {
						for p in &images {
							used.remove(p);
						}
					}

					let wallpaper = if display.shuffle {
						if let Some((ah, al)) = display_anchor {
							// Подбираем обои, близкие к первому экрану; если не нашлось — берём ближайшие
							pick_matching(&images, ah, al, 40.0, 20.0, &used)
								.or_else(|| pick_closest(&images, ah, al, &used))
						} else {
							pick_similar(&images, 40.0, 20.0, &used)
						}
					} else {
						images.iter().find(|p| !used.contains(*p)).cloned()
					};

					if let Some(path) = wallpaper {
						set_wallpaper(tool, &path, &display.name, &mode, transition)?;
						println!("{} {}", path.display(), display.name);
						if first_wallpaper.is_none() {
							first_wallpaper = Some(path.clone());
							display_anchor = average_hsl(&path);
						}
						used.insert(path);
					}
				}
				(None, Some(path)) => {
					let p = PathBuf::from(path);
					if !p.exists() {
						eprintln!("[{}] Файл не найден: {path}", display.name);
						continue;
					}
					set_wallpaper(tool, &p, &display.name, &mode, transition)?;
					println!("{} {}", path, display.name);
					if first_wallpaper.is_none() {
						first_wallpaper = Some(p);
					}
				}
				(None, None) => {
					eprintln!("[{}] Не указан ни path, ни dir", display.name);
					continue;
				}
			}
		}
	}

	save_used(&state, &used)?;
	Ok(first_wallpaper)
}
