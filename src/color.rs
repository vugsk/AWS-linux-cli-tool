use kmeans_color_gpu::RGBA8;

pub struct Hsl {
	pub h: f32, // 0–360
	pub s: f32, // 0–100
	pub l: f32, // 0–100
}

pub fn rgba8_to_hex(c: &RGBA8) -> String {
	format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

pub fn rgba8_to_hsl(c: &RGBA8) -> Hsl {
	let r = c.r as f32 / 255.0;
	let g = c.g as f32 / 255.0;
	let b = c.b as f32 / 255.0;

	let max = r.max(g).max(b);
	let min = r.min(g).min(b);
	let delta = max - min;

	let l = (max + min) / 2.0;
	let s = if delta == 0.0 {
		0.0
	} else {
		delta / (1.0 - (2.0 * l - 1.0).abs())
	};
	let h = if delta == 0.0 {
		0.0
	} else if max == r {
		60.0 * (((g - b) / delta) % 6.0)
	} else if max == g {
		60.0 * ((b - r) / delta + 2.0)
	} else {
		60.0 * ((r - g) / delta + 4.0)
	};

	Hsl {
		h: (h + 360.0) % 360.0,
		s: s * 100.0,
		l: l * 100.0,
	}
}

pub fn hsl_to_rgba8(h: f32, s: f32, l: f32) -> RGBA8 {
	let s = s / 100.0;
	let l = l / 100.0;
	let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
	let h_prime = h / 60.0;
	let x = c * (1.0 - (h_prime % 2.0 - 1.0).abs());
	let m = l - c / 2.0;
	let (r1, g1, b1) = if h_prime < 1.0 { (c, x, 0.0_f32) } else if h_prime < 2.0 { (x, c, 0.0) } else if h_prime < 3.0 { (0.0, c, x) } else if h_prime < 4.0 { (0.0, x, c) } else if h_prime < 5.0 { (x, 0.0, c) } else { (c, 0.0, x) };
	RGBA8 {
		r: ((r1 + m) * 255.0).clamp(0.0, 255.0) as u8,
		g: ((g1 + m) * 255.0).clamp(0.0, 255.0) as u8,
		b: ((b1 + m) * 255.0).clamp(0.0, 255.0) as u8,
		a: 255,
	}
}

/// Сдвигает яркость цвета к target_l, сохраняя оттенок и насыщенность
pub fn nudge_lightness(c: RGBA8, target_l: f32) -> RGBA8 {
	let hsl = rgba8_to_hsl(&c);
	hsl_to_rgba8(hsl.h, hsl.s, target_l.clamp(0.0, 100.0))
}

pub fn hue_distance(a: f32, b: f32) -> f32 {
	let d = (a - b).abs() % 360.0;
	if d > 180.0 { 360.0 - d } else { d }
}

// Относительная яркость по WCAG (0–1) — основа для расчёта контраста.
pub fn relative_luminance(c: &RGBA8) -> f32 {
	let f = |v: u8| {
		let v = v as f32 / 255.0;
		if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
	};
	0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
}

// Коэффициент контраста по WCAG: от 1:1 (одинаковые) до 21:1 (чёрный/белый).
pub fn contrast_ratio(a: &RGBA8, b: &RGBA8) -> f32 {
	let (la, lb) = (relative_luminance(a), relative_luminance(b));
	let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
	(hi + 0.05) / (lo + 0.05)
}

// Гарантирует контраст fg к bg не ниже target, двигая только светлоту (L в HSL)
// и сохраняя оттенок/насыщенность. Направление (к белому или чёрному) выбирается
// по тому, что даёт больший контраст с фоном; бинарный поиск — минимальный сдвиг.
pub fn ensure_contrast(fg: RGBA8, bg: &RGBA8, target: f32) -> RGBA8 {
	if contrast_ratio(&fg, bg) >= target {
		return fg;
	}
	let hsl = rgba8_to_hsl(&fg);
	let bg_lum = relative_luminance(bg);
	let lighten = (1.05 / (bg_lum + 0.05)) >= ((bg_lum + 0.05) / 0.05);

	let (mut lo, mut hi) = if lighten { (hsl.l, 100.0) } else { (0.0, hsl.l) };
	let mut best = if lighten { 100.0 } else { 0.0 };
	for _ in 0..24 {
		let mid = (lo + hi) / 2.0;
		if contrast_ratio(&hsl_to_rgba8(hsl.h, hsl.s, mid), bg) >= target {
			best = mid;
			if lighten { hi = mid; } else { lo = mid; }
		} else if lighten {
			lo = mid;
		} else {
			hi = mid;
		}
	}
	hsl_to_rgba8(hsl.h, hsl.s, best)
}

// «on-цвет»: читаемый текст для фона bg. Берём из палитры более контрастный из
// тёмного (CRUST) и светлого (TEXT); если ни один не дотягивает — чистый ч/б.
fn on_color(bg: &RGBA8, dark: RGBA8, light: RGBA8, target: f32) -> RGBA8 {
	let cd = contrast_ratio(&dark, bg);
	let cl = contrast_ratio(&light, bg);
	let (best, best_c) = if cd >= cl { (dark, cd) } else { (light, cl) };
	if best_c >= target {
		best
	} else if relative_luminance(bg) > 0.18 {
		RGBA8 { r: 0, g: 0, b: 0, a: 255 }
	} else {
		RGBA8 { r: 255, g: 255, b: 255, a: 255 }
	}
}

// sRGB → CIELAB (D65). Lab перцептивно равномерен, поэтому евклидово
// расстояние в нём (ΔE76) хорошо отражает «насколько цвета различимы глазом».
pub fn rgb_to_lab(rgb: [u8; 3]) -> (f32, f32, f32) {
	let lin = |c: u8| {
		let c = c as f32 / 255.0;
		if c > 0.04045 { ((c + 0.055) / 1.055).powf(2.4) } else { c / 12.92 }
	};
	let (r, g, b) = (lin(rgb[0]), lin(rgb[1]), lin(rgb[2]));

	// XYZ под белой точкой D65
	let x = (r * 0.4124 + g * 0.3576 + b * 0.1805) / 0.95047;
	let y = r * 0.2126 + g * 0.7152 + b * 0.0722;
	let z = (r * 0.0193 + g * 0.1192 + b * 0.9505) / 1.08883;

	let f = |t: f32| if t > 0.008856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
	let (fx, fy, fz) = (f(x), f(y), f(z));

	(116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

pub fn delta_e(a: (f32, f32, f32), b: (f32, f32, f32)) -> f32 {
	((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt()
}

// Доля цветов палитры `anchor`, у которых в `cand` есть близкий сосед
// (ΔE < delta_e_threshold). 1.0 — кандидат покрывает все цвета якоря
// (почти-дубль), 0.0 — палитры не пересекаются.
pub fn palette_similarity(anchor: &[[u8; 3]], cand: &[[u8; 3]], delta_e_threshold: f32) -> f32 {
	if anchor.is_empty() {
		return 0.0;
	}
	let cand_lab: Vec<(f32, f32, f32)> = cand.iter().map(|c| rgb_to_lab(*c)).collect();
	let matched = anchor
		.iter()
		.filter(|ca| {
			let la = rgb_to_lab(**ca);
			cand_lab.iter().any(|lb| delta_e(la, *lb) < delta_e_threshold)
		})
		.count();
	matched as f32 / anchor.len() as f32
}

// Роли, упорядоченные от тёмного к светлому — будут заполнены по яркости (L)
const DARK_ROLES: &[&str] = &[
	"CRUST", "MANTLE", "BASE",
	"SURFACE0", "SURFACE1", "SURFACE2",
	"OVERLAY0", "OVERLAY1", "OVERLAY2",
	"SUBTEXT0", "SUBTEXT1", "TEXT",
];

// Акцентные роли с целевым оттенком (Catppuccin Mocha как ориентир)
const ACCENT_ROLES: &[(&str, f32)] = &[
	("ROSEWATER", 8.0),
	("FLAMINGO", 0.0),
	("PINK", 316.0),
	("MAUVE", 267.0),
	("RED", 343.0),
	("MAROON", 350.0),
	("PEACH", 23.0),
	("YELLOW", 41.0),
	("GREEN", 115.0),
	("TEAL", 170.0),
	("SKY", 189.0),
	("SAPPHIRE", 199.0),
	("BLUE", 217.0),
	("LAVENDER", 232.0),
];

pub fn map_to_theme(
	palette: &[RGBA8],
	sat_threshold: f32,
	min_contrast: f32,
) -> Vec<(String, RGBA8)> {
	let with_hsl: Vec<(RGBA8, Hsl)> = palette.iter()
		.map(|c| (*c, rgba8_to_hsl(c)))
		.collect();

	// Структурные роли (CRUST→TEXT): все цвета по возрастанию яркости.
	// Фильтрация по насыщенности намеренно убрана — при насыщенной палитре
	// (синие, зелёные обои) achromatic-цветов слишком мало и градиент схлопывается.
	let mut by_lightness: Vec<&(RGBA8, Hsl)> = with_hsl.iter().collect();
	by_lightness.sort_by(|a, b| a.1.l.partial_cmp(&b.1.l).unwrap());

	// Акцентные роли: насыщенные цвета; если не хватает — добираем самые насыщенные из остальных
	let mut saturated: Vec<&(RGBA8, Hsl)> = with_hsl.iter()
		.filter(|(_, h)| h.s >= sat_threshold)
		.collect();

	if saturated.len() < 4 {
		let mut rest: Vec<&(RGBA8, Hsl)> = with_hsl.iter()
			.filter(|(_, h)| h.s < sat_threshold)
			.collect();
		rest.sort_by(|a, b| b.1.s.partial_cmp(&a.1.s).unwrap());
		let take = (4 - saturated.len()).min(rest.len());
		saturated.extend(rest.into_iter().take(take));
	}

	let mut result = Vec::new();
	let n = DARK_ROLES.len();

	// Равномерно покрываем шкалу яркости: CRUST = самый тёмный, TEXT = самый светлый
	for (i, &role) in DARK_ROLES.iter().enumerate() {
		let idx = if by_lightness.len() <= 1 {
			0
		} else {
			(i * (by_lightness.len() - 1)) / (n - 1)
		};
		result.push((role, by_lightness[idx.min(by_lightness.len() - 1)].0));
	}

	// Акцентные роли: берём цвет с оттенком, ближайшим к целевому
	for &(role, target_hue) in ACCENT_ROLES {
		if let Some((color, _)) = saturated.iter().min_by(|a, b| {
			hue_distance(a.1.h, target_hue)
				.partial_cmp(&hue_distance(b.1.h, target_hue))
				.unwrap()
		}) {
			result.push((role, *color));
		}
	}

	// Двойной барьер: относительный (от BASE) + абсолютный минимум яркости.
	// Нужен потому, что для очень тёмных палитр (BASE=L5) только относительный
	// порог даёт TEXT=L60 — математически 55 пунктов, но визуально недостаточно.
	// OVERLAY1 добавлен: ряд waybar-конфигов использует его для заголовка окна.
	let base_l = result.iter()
		.find(|(r, _)| *r == "BASE")
		.map(|(_, c)| rgba8_to_hsl(c).l)
		.unwrap_or(20.0);
	let dark_bg = base_l < 50.0;

	for (role, color) in result.iter_mut() {
		// (relative_min_from_base, absolute_floor_for_dark, absolute_ceil_for_light)
		let (rel, abs_dark, abs_light): (f32, f32, f32) = match *role {
			"TEXT" => (55.0, 78.0, 22.0),
			"SUBTEXT1" => (45.0, 66.0, 34.0),
			"SUBTEXT0" => (38.0, 56.0, 44.0),
			"OVERLAY2" => (28.0, 44.0, 56.0),
			"OVERLAY1" => (20.0, 34.0, 66.0),
			_ => continue,
		};
		let target_l = if dark_bg {
			(base_l + rel).max(abs_dark).min(95.0)
		} else {
			(base_l - rel).min(abs_light).max(5.0)
		};
		let cur_l = rgba8_to_hsl(color).l;
		let needs_nudge = if dark_bg { cur_l < target_l } else { cur_l > target_l };
		if needs_nudge {
			*color = nudge_lightness(*color, target_l);
		}
	}

	// Гарантия контраста: «передние» роли (текст + акценты) должны читаться на
	// фоне модулей (MANTLE). Двигаем только светлоту, оттенок сохраняем — поэтому
	// любой будущий модуль с акцентным текстом на mantle читаем по построению.
	let bg = result.iter().find(|(r, _)| *r == "MANTLE").map(|(_, c)| *c)
		.or_else(|| result.iter().find(|(r, _)| *r == "BASE").map(|(_, c)| *c))
		.unwrap_or(RGBA8 { r: 30, g: 30, b: 30, a: 255 });

	const FG_ROLES: &[&str] = &[
		"TEXT", "SUBTEXT1", "SUBTEXT0", "OVERLAY2",
		"ROSEWATER", "FLAMINGO", "PINK", "MAUVE", "RED", "MAROON", "PEACH",
		"YELLOW", "GREEN", "TEAL", "SKY", "SAPPHIRE", "BLUE", "LAVENDER",
	];
	for (role, color) in result.iter_mut() {
		if FG_ROLES.contains(role) {
			*color = ensure_contrast(*color, &bg, min_contrast);
		}
	}

	// on-цвета: гарантированно читаемый текст для каждой роли, когда она фон.
	let dark = result.iter().find(|(r, _)| *r == "CRUST").map(|(_, c)| *c)
		.unwrap_or(RGBA8 { r: 0, g: 0, b: 0, a: 255 });
	let light = result.iter().find(|(r, _)| *r == "TEXT").map(|(_, c)| *c)
		.unwrap_or(RGBA8 { r: 255, g: 255, b: 255, a: 255 });

	let mut out: Vec<(String, RGBA8)> = Vec::with_capacity(result.len() * 2);
	for (role, color) in &result {
		out.push((role.to_string(), *color));
		out.push((format!("{role}_ON"), on_color(color, dark, light, min_contrast)));
	}
	out
}
