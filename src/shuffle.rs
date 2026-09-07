//! 再生順序の組み立て。`core.py` の `ordered` 相当（非破壊・置換順列）。

use rand::Rng;

/// 入力の順序を変えずに、必要ならシャッフルしたコピーを返す。
pub fn ordered<T: Clone>(tracks: &[T], shuffle: bool, rng: &mut impl Rng) -> Vec<T> {
	let mut result = tracks.to_vec();
	if shuffle {
		use rand::seq::SliceRandom;
		result.shuffle(rng);
	}
	result
}

/// `chosen` と等価な最初の要素を先頭へ移動する。
/// 元リストに同じ曲が複数あっても、出現回数は変えない。
pub fn promote_to_front<T: PartialEq>(tracks: &mut Vec<T>, chosen: &T) {
	if let Some(pos) = tracks.iter().position(|t| t == chosen) {
		let item = tracks.remove(pos);
		tracks.insert(0, item);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use rand::SeedableRng;
	use rand::rngs::StdRng;

	#[test]
	fn shuffle_is_permutation_and_leaves_source() {
		let values: Vec<u32> = (0..50).collect();
		let out = ordered(&values, true, &mut StdRng::seed_from_u64(7));
		let mut sorted = out.clone();
		sorted.sort();
		assert_eq!(sorted, values);
		assert_ne!(out, values);
		assert_eq!(values, (0..50).collect::<Vec<u32>>());
	}

	#[test]
	fn ordered_without_shuffle_keeps_order() {
		let values = vec![3u32, 1, 2];
		assert_eq!(
			ordered(&values, false, &mut StdRng::seed_from_u64(1)),
			values
		);
	}

	#[test]
	fn selected_first_preserves_duplicate_occurrences() {
		// Java CoreTest.selectedFirstPreservesDuplicateOccurrences と同じ要求
		let source = vec!["A", "B", "A", "C"];
		let mut shuffled = ordered(&source, true, &mut StdRng::seed_from_u64(5));
		let chosen = source[2];
		promote_to_front(&mut shuffled, &chosen);
		assert_eq!(shuffled[0], "A");
		assert_eq!(shuffled.iter().filter(|t| **t == "A").count(), 2);
		assert_eq!(shuffled.len(), 4);
		assert_eq!(source, vec!["A", "B", "A", "C"]);
		let mut sorted = shuffled.clone();
		sorted.sort();
		let mut want = source.clone();
		want.sort();
		assert_eq!(sorted, want);
	}
}
