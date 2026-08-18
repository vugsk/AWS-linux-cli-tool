use std::path::PathBuf;

use crate::color::palette_similarity;

// Один обой со всем, что нужно для кластеризации.
pub struct Item {
	pub path: PathBuf,
	pub lightness: f32,
	pub palette: Vec<[u8; 3]>,
}

// Симметричная матрица сходства палитр (palette_similarity не симметрична,
// поэтому усредняем оба направления). n невелико (≈сотня) — O(n²) дёшево.
fn similarity_matrix(items: &[Item], delta_e: f32) -> Vec<Vec<f32>> {
	let n = items.len();
	let mut m = vec![vec![1.0f32; n]; n];
	for i in 0..n {
		for j in (i + 1)..n {
			let a = palette_similarity(&items[i].palette, &items[j].palette, delta_e);
			let b = palette_similarity(&items[j].palette, &items[i].palette, delta_e);
			let s = (a + b) / 2.0;
			m[i][j] = s;
			m[j][i] = s;
		}
	}
	m
}

// Среднее сходство элемента x с остальными членами группы
fn mean_sim(x: usize, group: &[usize], sim: &[Vec<f32>]) -> f32 {
	if group.len() <= 1 {
		return 1.0;
	}
	let s: f32 = group.iter().filter(|&&g| g != x).map(|&g| sim[x][g]).sum();
	s / (group.len() - 1) as f32
}

// Медоид: член группы с наибольшим средним сходством к остальным
fn medoid(group: &[usize], sim: &[Vec<f32>]) -> usize {
	*group
		.iter()
		.max_by(|&&a, &&b| mean_sim(a, group, sim).partial_cmp(&mean_sim(b, group, sim)).unwrap())
		.unwrap()
}

// Когерентность папки: среднее сходство всех членов с медоидом (0–1 элемент → 1.0)
pub fn cohesion(group: &[usize], sim: &[Vec<f32>]) -> f32 {
	if group.len() <= 1 {
		return 1.0;
	}
	let m = medoid(group, sim);
	let s: f32 = group.iter().map(|&g| sim[g][m]).sum();
	s / group.len() as f32
}

// Когерентность одной группы напрямую из items (для записи в folders.json,
// когда общей матрицы сходства уже нет под рукой). Группа мала — O(m²) дёшево.
pub fn cohesion_of(items: &[Item], group: &[usize], delta_e: f32) -> f32 {
	let m = group.len();
	if m <= 1 {
		return 1.0;
	}
	let local: Vec<Vec<f32>> = (0..m)
		.map(|a| {
			(0..m)
				.map(|b| {
					if a == b {
						1.0
					} else {
						(palette_similarity(&items[group[a]].palette, &items[group[b]].palette, delta_e)
							+ palette_similarity(&items[group[b]].palette, &items[group[a]].palette, delta_e))
							/ 2.0
					}
				})
				.collect()
		})
		.collect();
	let med = (0..m)
		.max_by(|&a, &b| {
			local[a].iter().sum::<f32>().partial_cmp(&local[b].iter().sum::<f32>()).unwrap()
		})
		.unwrap();
	local.iter().map(|row| row[med]).sum::<f32>() / m as f32
}

pub fn avg_lightness(group: &[usize], items: &[Item]) -> f32 {
	if group.is_empty() {
		return 0.0;
	}
	group.iter().map(|&g| items[g].lightness).sum::<f32>() / group.len() as f32
}

// k-медоиды при k=2: seed'ы — самая непохожая пара, далее назначение по
// ближайшему медоиду с пересчётом, 2–3 итерации. Если разделить не удалось
// (все ушли в один кластер) — второй кластер пустой.
fn split2(group: &[usize], sim: &[Vec<f32>]) -> (Vec<usize>, Vec<usize>) {
	if group.len() < 2 {
		return (group.to_vec(), Vec::new());
	}
	let (mut s1, mut s2) = (group[0], group[1]);
	let mut worst = f32::MAX;
	for i in 0..group.len() {
		for j in (i + 1)..group.len() {
			let s = sim[group[i]][group[j]];
			if s < worst {
				worst = s;
				s1 = group[i];
				s2 = group[j];
			}
		}
	}

	let (mut c1, mut c2) = (Vec::new(), Vec::new());
	for _ in 0..3 {
		c1.clear();
		c2.clear();
		for &g in group {
			if sim[g][s1] >= sim[g][s2] {
				c1.push(g);
			} else {
				c2.push(g);
			}
		}
		if c1.is_empty() || c2.is_empty() {
			break;
		}
		let (n1, n2) = (medoid(&c1, sim), medoid(&c2, sim));
		if n1 == s1 && n2 == s2 {
			break;
		}
		s1 = n1;
		s2 = n2;
	}
	(c1, c2)
}

// Основной алгоритм: квантильный seed по светлости → слияние соседей →
// выделение выбросов. Возвращает группы индексов items, упорядоченные по
// средней светлости.
pub fn recluster(items: &[Item], cohesion_min: f32, seed_k: usize, delta_e: f32) -> Vec<Vec<usize>> {
	let n = items.len();
	if n == 0 {
		return Vec::new();
	}
	let sim = similarity_matrix(items, delta_e);

	// 1. Квантильный seed: порядок по светлости, k групп ≈равного размера
	let mut order: Vec<usize> = (0..n).collect();
	order.sort_by(|&a, &b| items[a].lightness.partial_cmp(&items[b].lightness).unwrap());

	let k = seed_k.clamp(1, n);
	let mut groups: Vec<Vec<usize>> = (0..k)
		.map(|g| order[g * n / k..(g + 1) * n / k].to_vec())
		.collect();

	// 2a. Слияние с соседом по светлости + split2 (один проход слева направо)
	groups.sort_by(|a, b| avg_lightness(a, items).partial_cmp(&avg_lightness(b, items)).unwrap());
	let mut merged: Vec<Vec<usize>> = Vec::new();
	let mut i = 0;
	while i < groups.len() {
		let low = groups[i].len() > 1 && cohesion(&groups[i], &sim) < cohesion_min;
		if low && i + 1 < groups.len() {
			let mut pool = groups[i].clone();
			pool.extend_from_slice(&groups[i + 1]);
			let (a, b) = split2(&pool, &sim);
			if !a.is_empty() {
				merged.push(a);
			}
			if !b.is_empty() {
				merged.push(b);
			}
			i += 2;
		} else {
			merged.push(groups[i].clone());
			i += 1;
		}
	}
	groups = merged;

	// 2b. Выделение выбросов: пока папка ниже порога и в ней >2 обоев — убираем
	// самый непохожий на медоид; собранные выбросы группируем в новые папки.
	let mut outliers: Vec<usize> = Vec::new();
	for g in groups.iter_mut() {
		while g.len() > 2 && cohesion(g, &sim) < cohesion_min {
			let m = medoid(g, &sim);
			let worst_pos = (0..g.len())
				.filter(|&p| g[p] != m)
				.min_by(|&a, &b| sim[g[a]][m].partial_cmp(&sim[g[b]][m]).unwrap())
				.unwrap();
			outliers.push(g.remove(worst_pos));
		}
	}

	let mut new_folders: Vec<Vec<usize>> = Vec::new();
	for o in outliers {
		let mut placed = false;
		for nf in new_folders.iter_mut() {
			if sim[o][medoid(nf, &sim)] >= cohesion_min {
				nf.push(o);
				placed = true;
				break;
			}
		}
		if !placed {
			new_folders.push(vec![o]);
		}
	}
	groups.extend(new_folders);

	// 3. Финальный порядок по средней светлости
	groups.retain(|g| !g.is_empty());
	groups.sort_by(|a, b| avg_lightness(a, items).partial_cmp(&avg_lightness(b, items)).unwrap());
	groups
}
