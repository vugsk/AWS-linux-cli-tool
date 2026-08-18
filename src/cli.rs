use std::path::PathBuf;
use std::process::Command;

use crate::cache;
use crate::color::{map_to_theme, rgba8_to_hex};
use crate::generation::scaffold_custom_generator;
use crate::palette::extract_palette;
use crate::sort;
use crate::state::{config_path, expand_path, load_config};

// Возвращает true, если команда обработана (основной режим запускать не нужно),
// и false, если аргументов нет — тогда main запускает обычный пайплайн.
pub fn run() -> Result<bool, Box<dyn std::error::Error>> {
	let args: Vec<String> = std::env::args().skip(1).collect();
	let Some(cmd) = args.first() else {
		return Ok(false);
	};

	match cmd.as_str() {
		"palette" => cmd_palette(&args[1..])?,
		"info" | "status" => cmd_info()?,
		"list" => cmd_list()?,
		"folders" => cmd_folders()?,
		"new-generator" | "new-gen" => cmd_new_generator(&args[1..])?,
		"help" | "-h" | "--help" => print_help(),
		other => {
			eprintln!("Неизвестная команда: «{other}»\n");
			print_help();
		}
	}
	Ok(true)
}

// ── Команды ──────────────────────────────────────────────────────────────

// palette <изображение> [--colors N] [--no-map]
// Извлекает палитру из произвольной картинки и печатает в терминал. На систему
// не влияет — чисто информационно (подобрать кол-во цветов, посмотреть результат).
fn cmd_palette(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
	let mut image: Option<String> = None;
	let mut colors: Option<u32> = None;
	let mut show_map = true;

	let mut i = 0;
	while i < args.len() {
		match args[i].as_str() {
			"--colors" | "-c" => {
				i += 1;
				colors = Some(
					args.get(i)
						.ok_or("--colors требует число")?
						.parse()
						.map_err(|_| "--colors: ожидалось целое число")?,
				);
			}
			"--no-map" => show_map = false,
			other if !other.starts_with('-') && image.is_none() => {
				image = Some(other.to_string());
			}
			other => return Err(format!("Неизвестный аргумент palette: «{other}»").into()),
		}
		i += 1;
	}

	let image = image.ok_or(
		"Укажите путь к изображению: aswadpftls palette <картинка> [--colors N] [--no-map]",
	)?;
	let config = load_config().unwrap_or_default();
	let n = colors.unwrap_or(config.palette.color);

	let palette = extract_palette(&PathBuf::from(&image), n)?;

	println!("Доминирующие цвета ({n}) — {image}:");
	for c in &palette {
		println!("  {}  {}", swatch(c.r, c.g, c.b), rgba8_to_hex(c));
	}

	if show_map {
		let mapping = map_to_theme(
			&palette,
			config.palette.sat_threshold as f32,
			config.palette.min_contrast,
		);
		println!("\nРоли темы (как легли бы в палитру):");
		for (role, c) in &mapping {
			println!("  {}  COLOR_{:<10} {}", swatch(c.r, c.g, c.b), role, rgba8_to_hex(c));
		}
	}
	Ok(())
}

// info — текущее состояние: пути, инструмент, текущие обои и активная палитра.
fn cmd_info() -> Result<(), Box<dyn std::error::Error>> {
	let config = load_config().unwrap_or_default();

	println!("Конфиг:        {}", config_path().display());
	println!("Инструмент:    {}", config.wallpaper.tool);
	println!("Режим:         {}", config.wallpaper.mode);
	if let Some(store) = &config.wallpaper.store {
		println!("Хранилище:     {}", expand_path(store).display());
	}
	if let Some(dir) = &config.wallpaper.dir {
		println!("Директория:    {}", expand_path(dir).display());
	}
	let palette_path = expand_path(&config.palette.output);
	println!("Файл палитры:  {}", palette_path.display());

	println!("\nТекущие обои:");
	match query_wallpaper(&config.wallpaper.tool) {
		Some(out) if !out.trim().is_empty() => {
			for line in out.lines() {
				println!("  {line}");
			}
		}
		_ => println!("  (не удалось получить от «{}»)", config.wallpaper.tool),
	}

	println!("\nАктивная палитра ({}):", palette_path.display());
	match std::fs::read_to_string(&palette_path) {
		Ok(content) => {
			for (role, hex) in parse_palette_fish(&content) {
				if let Some((r, g, b)) = hex_to_rgb(&hex) {
					println!("  {}  COLOR_{:<12} {hex}", swatch(r, g, b), role);
				} else {
					println!("     COLOR_{role:<12} {hex}");
				}
			}
		}
		Err(_) => println!("  (файл палитры ещё не создан)"),
	}
	Ok(())
}

// list — обои из хранилища с их кэшированной светлостью и мини-палитрой.
fn cmd_list() -> Result<(), Box<dyn std::error::Error>> {
	let config = load_config().unwrap_or_default();
	let Some(store) = &config.wallpaper.store else {
		println!("Режим хранилища выключен (wallpaper.store не задан).");
		return Ok(());
	};
	let store = expand_path(store);
	let cache = cache::load();

	let mut rows: Vec<(&String, &cache::Entry)> = cache
		.iter()
		.filter(|(p, _)| p.starts_with(&*store.to_string_lossy()))
		.collect();
	if rows.is_empty() {
		println!("В кэше нет обоев для {}. Запустите программу без аргументов для синхронизации.", store.display());
		return Ok(());
	}
	rows.sort_by(|a, b| a.1.lightness.partial_cmp(&b.1.lightness).unwrap_or(std::cmp::Ordering::Equal));

	println!("Обои в хранилище {} ({} шт.):", store.display(), rows.len());
	for (path, entry) in rows {
		let name = PathBuf::from(path)
			.file_name()
			.map(|n| n.to_string_lossy().into_owned())
			.unwrap_or_else(|| path.clone());
		let swatches: String = entry
			.palette
			.iter()
			.take(6)
			.map(|c| swatch(c[0], c[1], c[2]))
			.collect();
		println!("  L={:>3.0}  {}  {}", entry.lightness, swatches, name);
	}
	Ok(())
}

// folders — кластеры хранилища (folders.json): светлость, когезия, размер.
fn cmd_folders() -> Result<(), Box<dyn std::error::Error>> {
	let config = load_config().unwrap_or_default();
	let Some(store) = &config.wallpaper.store else {
		println!("Режим хранилища выключен (wallpaper.store не задан).");
		return Ok(());
	};
	let store = expand_path(store);
	let folders = sort::load_folders(&store);
	if folders.is_empty() {
		println!("Папок ещё нет ({}). Запустите программу для сортировки.", store.display());
		return Ok(());
	}
	println!("Папки-кластеры в {}:", store.display());
	println!("  {:<16} {:>6} {:>9} {:>7}", "имя", "L", "когезия", "обоев");
	for f in &folders {
		println!(
			"  {:<16} {:>6.0} {:>9.2} {:>7}",
			f.name,
			f.avg_lightness,
			f.cohesion,
			f.members.len()
		);
	}
	Ok(())
}

// new-generator <name> — создаёт заготовку кастомного генератора.
fn cmd_new_generator(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
	let name = args
		.first()
		.ok_or("Укажите имя: aswadpftls new-generator <name>")?;
	let config = load_config().unwrap_or_default();
	let path = scaffold_custom_generator(&config.behavior.scripts_dir, name)?;
	println!("Создан кастомный генератор: {}", path.display());
	println!(
		"Дальше:\n  1. Отредактируйте файл под нужную программу.\n  \
         2. Добавьте \"{name}\" в behavior.generation_conf в {}.",
		config_path().display()
	);
	Ok(())
}

fn print_help() {
	println!(
		"ASWaDPFTLS — авто-обои и палитра темы.\n\n\
         Использование:\n  \
         aswadpftls                     применить обои и тему (основной режим)\n  \
         aswadpftls palette <img> [--colors N] [--no-map]\n  \
         {sp}извлечь палитру из картинки и показать (на систему не влияет)\n  \
         aswadpftls info                показать пути, текущие обои и активную палитру\n  \
         aswadpftls list                обои хранилища со светлостью и мини-палитрой\n  \
         aswadpftls folders             кластеры хранилища (светлость, когезия, размер)\n  \
         aswadpftls new-generator <name>  создать заготовку кастомного генератора\n  \
         aswadpftls help                эта справка",
		sp = " ".repeat(33)
	);
}

// ── Вспомогательное ──────────────────────────────────────────────────────

// Цветной квадрат через ANSI truecolor (фон).
fn swatch(r: u8, g: u8, b: u8) -> String {
	format!("\x1b[48;2;{r};{g};{b}m   \x1b[0m")
}

fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
	let h = hex.trim_start_matches('#');
	if h.len() != 6 {
		return None;
	}
	let r = u8::from_str_radix(&h[0..2], 16).ok()?;
	let g = u8::from_str_radix(&h[2..4], 16).ok()?;
	let b = u8::from_str_radix(&h[4..6], 16).ok()?;
	Some((r, g, b))
}

// Разбирает строки вида: set -gx COLOR_BASE "#1e1e1e"
fn parse_palette_fish(content: &str) -> Vec<(String, String)> {
	content
		.lines()
		.filter_map(|line| {
			let rest = line.trim().strip_prefix("set -gx COLOR_")?;
			let (role, tail) = rest.split_once(char::is_whitespace)?;
			let hex = tail.trim().trim_matches('"').to_string();
			Some((role.to_string(), hex))
		})
		.collect()
}

// Спрашивает текущие обои у инструмента (swww/awww умеют `query`).
fn query_wallpaper(tool: &str) -> Option<String> {
	if !matches!(tool, "awww" | "swww") {
		return None;
	}
	let out = Command::new(tool).arg("query").output().ok()?;
	out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}
