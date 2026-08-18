use std::{fs, path::PathBuf, collections::HashSet};
use toml::from_str;

use crate::config::Config;

pub fn expand_path(path: &str) -> PathBuf {
	if let Some(rest) = path.strip_prefix("~/") {
		let home = std::env::var("HOME").unwrap_or_default();
		PathBuf::from(home).join(rest)
	} else {
		PathBuf::from(path)
	}
}

pub fn config_path() -> PathBuf {
	let home = std::env::var("HOME").unwrap_or_default();
	PathBuf::from(home).join(".config").join("colors").join("config.toml")
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
	let path = config_path();
	match fs::read_to_string(&path) {
		Ok(content) => Ok(from_str(&content)?),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
			let config = Config::default();
			if let Some(parent) = path.parent() {
				fs::create_dir_all(parent)?;
			}
			fs::write(&path, toml::to_string(&config)?)?;
			eprintln!("Конфиг не найден, создан по умолчанию: {}", path.display());
			Ok(config)
		}
		Err(e) => Err(Box::new(e)),
	}
}

pub fn state_path() -> PathBuf {
	let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
	PathBuf::from(home).join(".cache").join("aswadpftls").join("used.txt")
}

pub fn load_used(path: &PathBuf) -> HashSet<PathBuf> {
	fs::read_to_string(path)
		.unwrap_or_default()
		.lines()
		.filter(|l| !l.is_empty())
		.map(PathBuf::from)
		.collect()
}

pub fn save_used(path: &PathBuf, used: &HashSet<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)?;
	}
	let content = used.iter()
		.map(|p| p.to_string_lossy().into_owned())
		.collect::<Vec<_>>()
		.join("\n");
	fs::write(path, content)?;
	Ok(())
}
