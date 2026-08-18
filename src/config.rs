use serde::{Deserialize, Serialize};

fn default_tool() -> String { "awww".to_string() }
fn default_mode() -> String { "fill".to_string() }
fn default_palette_color() -> u32 { 20 }
fn default_sat_threshold() -> u32 { 35 }
fn default_palette_output() -> String { "~/.config/colors/palette.fish".to_string() }
fn default_match_min() -> f32 { 0.80 }
fn default_match_max() -> f32 { 1.00 }
fn default_match_delta_e() -> f32 { 15.0 }
fn default_folder_cohesion_min() -> f32 { 0.60 }
fn default_min_contrast() -> f32 { 3.0 }
fn default_lightness_window() -> f32 { 12.0 }
fn default_seed_buckets() -> u32 { 5 }
fn default_scripts_dir() -> String { "~/.config/colors/scripts/".to_string() }
fn default_transition_kind() -> String { "fade".to_string() }
fn default_transition_duration() -> f32 { 1.0 }
fn default_transition_fps() -> u32 { 60 }

#[derive(Deserialize, Serialize)]
pub struct Config {
	#[serde(default)]
	pub wallpaper: Wallpaper,
	#[serde(default)]
	pub palette: Palette,
	#[serde(default)]
	pub behavior: Behavior,
	#[serde(default)]
	pub display: Vec<Display>,
}

impl Default for Config {
	fn default() -> Self {
		Self {
			wallpaper: Wallpaper::default(),
			palette: Palette::default(),
			behavior: Behavior::default(),
			display: Vec::new(),
		}
	}
}

#[derive(Deserialize, Serialize)]
pub struct Wallpaper {
	#[serde(skip_serializing_if = "Option::is_none")]
	pub path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub dir: Option<String>,
	// Если задан — включается режим хранилища с динамическими папками по
	// сходству палитр (Pictures/colors), подбором по времени суток и экранам.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub store: Option<String>,
	// Полуширина диапазона светлости при выборе папки по времени суток:
	// время → целевая L, кандидаты — папки с avg_lightness в [L−W, L+W].
	#[serde(default = "default_lightness_window")]
	pub lightness_window: f32,
	// Число начальных квантильных групп по светлости (seed кластеризации)
	#[serde(default = "default_seed_buckets")]
	pub seed_buckets: u32,
	#[serde(default = "default_tool")]
	pub tool: String,
	#[serde(default = "default_mode")]
	pub mode: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub transition: Option<Transition>,
}

impl Default for Wallpaper {
	fn default() -> Self {
		Self {
			path: None,
			dir: None,
			store: None,
			lightness_window: default_lightness_window(),
			seed_buckets: default_seed_buckets(),
			tool: default_tool(),
			mode: default_mode(),
			transition: Some(Transition::default()),
		}
	}
}

#[derive(Deserialize, Serialize)]
pub struct Transition {
	#[serde(rename = "type", default = "default_transition_kind")]
	pub kind: String,
	#[serde(default = "default_transition_duration")]
	pub duration: f32,
	#[serde(default = "default_transition_fps")]
	pub fps: u32,
}

impl Default for Transition {
	fn default() -> Self {
		Self {
			kind: default_transition_kind(),
			duration: default_transition_duration(),
			fps: default_transition_fps(),
		}
	}
}

#[derive(Deserialize, Serialize)]
pub struct Palette {
	#[serde(default = "default_palette_color")]
	pub color: u32,
	#[serde(default = "default_sat_threshold")]
	pub sat_threshold: u32,
	#[serde(default = "default_palette_output")]
	pub output: String,
	// Подбор обоев для 2+ экранов: сходство палитр должно быть в [min, max).
	// По умолчанию [0.80, 1.00) — согласованно, но не дубль.
	#[serde(default = "default_match_min")]
	pub match_min: f32,
	#[serde(default = "default_match_max")]
	pub match_max: f32,
	// Порог ΔE (Lab): два цвета считаются «совпавшими», если различие ниже него
	#[serde(default = "default_match_delta_e")]
	pub match_delta_e: f32,
	// Минимальная когерентность папки (среднее сходство с медоидом). Папки ниже
	// порога переразбиваются: слияние с соседом, затем выделение выбросов.
	#[serde(default = "default_folder_cohesion_min")]
	pub folder_cohesion_min: f32,
	// Минимальный контраст (WCAG) текста/акцентов к фону модулей. Цвета ниже
	// порога подсветляются/затемняются. 3.0 — мягко, 4.5 — AA, 7.0 — AAA.
	#[serde(default = "default_min_contrast")]
	pub min_contrast: f32,
}

impl Default for Palette {
	fn default() -> Self {
		Self {
			color: default_palette_color(),
			sat_threshold: default_sat_threshold(),
			output: default_palette_output(),
			match_min: default_match_min(),
			match_max: default_match_max(),
			match_delta_e: default_match_delta_e(),
			folder_cohesion_min: default_folder_cohesion_min(),
			min_contrast: default_min_contrast(),
		}
	}
}

#[derive(Deserialize, Serialize)]
pub struct Behavior {
	#[serde(default)]
	pub generation: bool,
	#[serde(default)]
	pub generation_conf: Vec<String>,
	#[serde(default = "default_scripts_dir")]
	pub scripts_dir: String,
	#[serde(default)]
	pub dry_run: bool,
}

impl Default for Behavior {
	fn default() -> Self {
		Self {
			generation: false,
			generation_conf: Vec::new(),
			scripts_dir: default_scripts_dir(),
			dry_run: false,
		}
	}
}

#[derive(Deserialize, Serialize)]
pub struct Display {
	pub name: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub dir: Option<String>,
	#[serde(default = "default_mode")]
	pub mode: String,
	#[serde(default)]
	pub shuffle: bool,
}
