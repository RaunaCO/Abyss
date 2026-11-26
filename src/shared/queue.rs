//! Queue order helpers (shuffle, restoring the original order, the start row).

use std::collections::HashMap;

use super::{now_secs, QItem};

/// Where `list[start]` ends up once unplayable tracks are removed (`playable`
/// flags each track), or `None` if that track is itself unplayable.
pub fn playable_start(playable: &[bool], start: usize) -> Option<usize> {
    if !*playable.get(start)? {
        return None;
    }
    Some(playable[..start].iter().filter(|p| **p).count())
}

/// The queue in its original order again (shuffle turned off), keeping what
/// was added or removed while it was shuffled: entries are matched by UID and
/// newcomers follow in their live order.
pub fn restore_queue_order(original: Vec<QItem>, live: Vec<QItem>) -> Vec<QItem> {
    let mut by_uid: HashMap<_, _> = live.into_iter().enumerate().map(|(index, item)| (item.uid, (index, item))).collect();
    let mut restored: Vec<_> = original.into_iter().filter_map(|item| by_uid.remove(&item.uid).map(|(_, live)| live)).collect();
    let mut appended: Vec<_> = by_uid.into_values().collect();
    appended.sort_unstable_by_key(|(index, _)| *index);
    restored.extend(appended.into_iter().map(|(_, item)| item));
    restored
}

/// Fisher-Yates with a xorshift seeded from the clock.
pub fn shuffle<T>(v: &mut [T]) {
    let mut seed = now_secs() | 1;
    for i in (1..v.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let j = (seed % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
}

#[cfg(test)]
mod play_start_tests {
    use super::{playable_start, restore_queue_order};
    use crate::shared::QItem;

    #[test]
    fn disabling_shuffle_keeps_live_additions_and_removals() {
        let item = |uid| QItem { uid, track: crate::tidal::Track::default() };
        let original = vec![item(1), item(2), item(3)];
        let live = vec![item(3), item(4), item(1), item(5)];
        let order: Vec<_> = restore_queue_order(original, live).into_iter().map(|q| q.uid).collect();
        assert_eq!(order, [1, 3, 4, 5]);
    }

    #[test]
    fn picked_track_survives_unplayable_ones_above() {
        // Two unavailable tracks above row 3: the queue must still start on row 3.
        let flags = [true, false, false, true, true];
        assert_eq!(playable_start(&flags, 3), Some(1));
        assert_eq!(playable_start(&flags, 4), Some(2));
        assert_eq!(playable_start(&flags, 0), Some(0));
    }

    #[test]
    fn unplayable_or_missing_track_plays_nothing() {
        assert_eq!(playable_start(&[true, false], 1), None);
        assert_eq!(playable_start(&[true], 5), None);
    }
}
