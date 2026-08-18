use std::{fs, path::{Path, PathBuf}};

use image::ImageReader;
use kmeans_color_gpu::image::Image;
use kmeans_color_gpu::{Algorithm, ImageProcessor};
use serde::{Deserialize, Serialize};

use crate::cache::{self, Cache, Entry};
use crate::cluster::{self, Item};
use crate::config::Config;
use crate::wallpaper::average_hsl;

// Метаданные папки-кластера, сохраняются в <store>/folders.json.
#[derive(Serialize, Deserialize, Clone)]
pub struct FolderMeta {
	pub name: String,
	pub avg_lightness: f32,
	pub cohesion: f32,
	pub members: Vec<String>,
}

pub fn inbox_dir(store: &Path) -> PathBuf {
	store.join("dirs")
}

pub fn folders_path(store: &Path) -> PathBuf {
	store.join("folders.json")
}

fn staging_dir(store: &Path) -> PathBuf {
	store.join(".aswadpftls_new")
}

fn is_image(p: &Path) -> bool {
	matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("png" | "jpg" | "jpeg" | "webp")
    )
}

fn images_in(dir: &Path) -> Vec<PathBuf> {
	fs::read_dir(dir)
		.map(|entries| {
			entries
				.filter_map(|e| e.ok())
				.map(|e| e.path())
				.filter(|p| is_image(p))
				.collect()
		})
		.unwrap_or_default()
}

// Папки-кластеры: все подкаталоги store, кроме входящего dirs/ и staging.
fn cluster_dirs(store: &Path) -> Vec<PathBuf> {
	let staging = staging_dir(store);
	fs::read_dir(store)
		.map(|entries| {
			entries
				.filter_map(|e| e.ok())
				.map(|e| e.path())
				.filter(|p| {
					p.is_dir()
						&& p.file_name().and_then(|n| n.to_str()) != Some("dirs")
						&& *p != staging
				})
				.collect()
		})
		.unwrap_or_default()
}

// Подбирает имя назначения, не затирая существующий файл
fn unique_dest(dir: &Path, src: &Path) -> PathBuf {
	let name = src.file_name().unwrap_or_default();
	let dst = dir.join(name);
	if !dst.exists() {
		return dst;
	}
	let stem = src.file_stem().and_then(|s| s.to_str()).unwrap_or("img");
	let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("png");
	for n in 1.. {
		let cand = dir.join(format!("{stem}_{n}.{ext}"));
		if !cand.exists() {
			return cand;
		}
	}
	unreachable!()
}

// Перемещение с фолбэком на копирование+удаление (на случай разных ФС, EXDEV)
fn move_file(src: &Path, dst: &Path) -> std::io::Result<()> {
	fs::rename(src, dst).or_else(|_| {
		fs::copy(src, dst)?;
		fs::remove_file(src)
	})
}

fn palette_of(
	proc: &ImageProcessor,
	path: &Path,
	num_colors: u32,
) -> Result<Vec<[u8; 3]>, Box<dyn std::error::Error>> {
	// Уменьшаем до 1024 px по большей стороне: текстура WGPU ограничена 8192,
	// а для извлечения доминирующих цветов полное разрешение не нужно.
	let img_buffer = ImageReader::open(path)?.decode()?.thumbnail(1024, 1024).to_rgba8();
	let dimensions = img_buffer.dimensions();
	let img = Image::new(dimensions, bytemuck::cast_slice(img_buffer.as_raw()));
	let pal = pollster::block_on(proc.palette(num_colors, &img, Algorithm::Kmeans))?;
	Ok(pal.iter().map(|c| [c.r, c.g, c.b]).collect())
}

// Досчитывает палитру и яркость для всех изображений, которых нет в кэше или у
// которых изменился mtime/нет яркости. GPU-процессор инициализируется один раз.
fn sync_cache(paths: &[PathBuf], num_colors: u32, cache: &mut Cache) -> usize {
	let stale: Vec<&PathBuf> = paths
		.iter()
		.filter(|p| match cache.get(p.to_string_lossy().as_ref()) {
			Some(e) => e.lightness < 0.0 || e.mtime != cache::mtime_of(p),
			None => true,
		})
		.collect();

	if stale.is_empty() {
		return 0;
	}

	let proc = match pollster::block_on(ImageProcessor::new()) {
		Ok(p) => p,
		Err(e) => {
			eprintln!("GPU-инициализация для палитр не удалась: {e}");
			return 0;
		}
	};

	let mut done = 0;
	for p in stale {
		let lightness = average_hsl(p).map(|(_, l)| l).unwrap_or(-1.0);
		match palette_of(&proc, p, num_colors) {
			Ok(palette) => {
				cache.insert(
					p.to_string_lossy().into_owned(),
					Entry { mtime: cache::mtime_of(p), lightness, palette },
				);
				done += 1;
			}
			Err(e) => eprintln!("Палитра не извлечена {}: {e}", p.display()),
		}
	}
	done
}

pub fn load_folders(store: &Path) -> Vec<FolderMeta> {
	fs::read_to_string(folders_path(store))
		.ok()
		.and_then(|s| serde_json::from_str(&s).ok())
		.unwrap_or_default()
}

fn save_folders(store: &Path, metas: &[FolderMeta]) -> std::io::Result<()> {
	let data = serde_json::to_string_pretty(metas).unwrap_or_else(|_| "[]".to_string());
	fs::write(folders_path(store), data)
}

// Раскладывает файлы по новым папкам fNN через staging, чтобы не конфликтовать
// со старыми папками, затем удаляет старые и переносит fNN в store. Обновляет
// ключи кэша на финальные пути и возвращает метаданные папок.
fn apply_layout(
	store: &Path,
	items: &[Item],
	groups: &[Vec<usize>],
	delta_e: f32,
	cache: &mut Cache,
) -> Result<Vec<FolderMeta>, Box<dyn std::error::Error>> {
	let staging = staging_dir(store);
	let _ = fs::remove_dir_all(&staging);
	fs::create_dir_all(&staging)?;

	// (старый ключ кэша, имя папки, итоговое имя файла)
	let mut rekey: Vec<(String, String, std::ffi::OsString)> = Vec::new();
	let mut metas: Vec<FolderMeta> = Vec::new();

	for (gi, group) in groups.iter().enumerate() {
		let name = format!("f{gi:02}");
		let gdir = staging.join(&name);
		fs::create_dir_all(&gdir)?;
		for &idx in group {
			let src = &items[idx].path;
			let dst = unique_dest(&gdir, src);
			move_file(src, &dst)?;
			let basename = dst.file_name().unwrap_or_default().to_owned();
			rekey.push((src.to_string_lossy().into_owned(), name.clone(), basename));
		}
		metas.push(FolderMeta {
			name,
			avg_lightness: cluster::avg_lightness(group, items),
			cohesion: cluster::cohesion_of(items, group, delta_e),
			members: Vec::new(),
		});
	}

	// Удаляем старые папки-кластеры (b0…b4 и прежние fNN)
	for d in cluster_dirs(store) {
		let _ = fs::remove_dir_all(&d);
	}

	// Переносим staging/fNN → store/fNN
	for meta in &metas {
		let from = staging.join(&meta.name);
		let to = store.join(&meta.name);
		let _ = fs::remove_dir_all(&to);
		fs::rename(&from, &to)?;
	}
	let _ = fs::remove_dir_all(&staging);

	// Перекладываем ключи кэша на финальные пути и заполняем members
	let mut members_by_folder: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
	for (old_key, folder, basename) in rekey {
		let final_path = store.join(&folder).join(&basename);
		let final_key = final_path.to_string_lossy().into_owned();
		if let Some(entry) = cache.remove(&old_key) {
			cache.insert(final_key.clone(), entry);
		}
		members_by_folder.entry(folder).or_default().push(final_key);
	}
	for meta in &mut metas {
		if let Some(m) = members_by_folder.remove(&meta.name) {
			meta.members = m;
		}
	}

	Ok(metas)
}

// Запускается при каждом старте, но реальную работу делает только если в dirs/
// появились обои или ещё нет folders.json. Возвращает true, если пересортировал.
pub fn run_sort(
	store: &Path,
	config: &Config,
	cache: &mut Cache,
) -> Result<bool, Box<dyn std::error::Error>> {
	let inbox_imgs = images_in(&inbox_dir(store));
	let folders_valid = !load_folders(store).is_empty();

	if inbox_imgs.is_empty() && folders_valid {
		return Ok(false);
	}

	// Все обои: папки-кластеры + входящие dirs/
	let mut all: Vec<PathBuf> = cluster_dirs(store).iter().flat_map(|d| images_in(d)).collect();
	all.extend(inbox_imgs);

	let synced = sync_cache(&all, config.palette.color, cache);
	if synced > 0 {
		println!("Палитр посчитано: {synced}");
	}

	// Item'ы только для тех, у кого есть кэш (палитра + яркость)
	let items: Vec<Item> = all
		.iter()
		.filter_map(|p| {
			cache.get(p.to_string_lossy().as_ref()).map(|e| Item {
				path: p.clone(),
				lightness: e.lightness,
				palette: e.palette.clone(),
			})
		})
		.collect();

	if items.is_empty() {
		save_folders(store, &[])?;
		return Ok(true);
	}

	let groups = cluster::recluster(
		&items,
		config.palette.folder_cohesion_min,
		config.wallpaper.seed_buckets as usize,
		config.palette.match_delta_e,
	);

	let metas = apply_layout(store, &items, &groups, config.palette.match_delta_e, cache)?;
	cache.retain(|k, _| Path::new(k).exists());
	save_folders(store, &metas)?;

	println!(
		"Папок после кластеризации: {} (cohesion: {})",
		metas.len(),
		metas.iter().map(|m| format!("{:.2}", m.cohesion)).collect::<Vec<_>>().join(" ")
	);
	Ok(true)
}
