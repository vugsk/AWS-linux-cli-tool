use std::{collections::HashMap, fs, path::{Path, PathBuf}, time::UNIX_EPOCH};
use serde::{Deserialize, Serialize};

// Кэш палитр: путь к обоям → время изменения файла + извлечённая палитра.
// Палитра считается GPU-k-means один раз при синхронизации; на выборе экранов
// мы только читаем кэш, поэтому подбор по сходству палитр остаётся дешёвым.
fn uncomputed_lightness() -> f32 { -1.0 }

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
	pub mtime: u64,
	// Средняя яркость (L в HSL, 0–100) — для квантильного seed по светлости.
	// Сентинел -1.0 → запись из старого кэша без яркости, считается устаревшей.
	#[serde(default = "uncomputed_lightness")]
	pub lightness: f32,
	pub palette: Vec<[u8; 3]>,
}

pub type Cache = HashMap<String, Entry>;

pub fn cache_path() -> PathBuf {
	let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
	PathBuf::from(home).join(".cache").join("aswadpftls").join("palettes.json")
}

pub fn load() -> Cache {
	fs::read_to_string(cache_path())
		.ok()
		.and_then(|s| serde_json::from_str(&s).ok())
		.unwrap_or_default()
}

pub fn save(cache: &Cache) -> std::io::Result<()> {
	let path = cache_path();
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}
	let data = serde_json::to_string(cache).unwrap_or_else(|_| "{}".to_string());
	fs::write(path, data)
}

// Время модификации файла в секундах эпохи — ключ инвалидации кэша
pub fn mtime_of(path: &Path) -> u64 {
	fs::metadata(path)
		.and_then(|m| m.modified())
		.ok()
		.and_then(|t| t.duration_since(UNIX_EPOCH).ok())
		.map(|d| d.as_secs())
		.unwrap_or(0)
}
