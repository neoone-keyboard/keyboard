//! Where the letters sit on the QWERTY layout, so a typo between two
//! neighbouring keys costs less than one between keys far apart.

const ROWS: [(&[u8], f32); 3] = [(b"qwertyuiop", 0.0), (b"asdfghjkl", 0.5), (b"zxcvbnm", 1.5)];

/// Centre of a lowercase letter's key, in key widths.
fn position(letter: u8) -> Option<(f32, f32)> {
    ROWS.iter().enumerate().find_map(|(row, (keys, offset))| {
        keys.iter()
            .position(|&k| k == letter)
            .map(|col| (col as f32 + offset, row as f32))
    })
}

/// True when two different letters are next to each other on the keyboard
/// (same row or the row above or below).
pub fn adjacent(a: u8, b: u8) -> bool {
    if a == b {
        return false;
    }
    match (
        position(a.to_ascii_lowercase()),
        position(b.to_ascii_lowercase()),
    ) {
        (Some((ax, ay)), Some((bx, by))) => {
            let (dx, dy) = (ax - bx, ay - by);
            dx * dx + dy * dy <= 1.6 * 1.6
        }
        _ => false,
    }
}

/// `table[a][..]` as a bit set: bit `b` is set when letters `a` and `b`
/// (0 = 'a') are neighbours. Built once per query so the hot loop is a lookup.
pub fn adjacency_table() -> [u32; 26] {
    let mut table = [0u32; 26];
    for (a, row) in table.iter_mut().enumerate() {
        for b in 0..26u8 {
            if adjacent(b'a' + a as u8, b'a' + b) {
                *row |= 1 << b;
            }
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use super::adjacent;

    #[test]
    fn neighbours_on_the_same_row() {
        assert!(adjacent(b'q', b'w'));
        assert!(adjacent(b'g', b'h'));
        assert!(!adjacent(b'q', b'e'));
    }

    #[test]
    fn neighbours_across_rows() {
        assert!(adjacent(b'e', b'd'));
        assert!(adjacent(b's', b'w'));
        assert!(adjacent(b'b', b'g'));
        assert!(adjacent(b'n', b'j'));
        assert!(!adjacent(b'q', b'z'));
        assert!(!adjacent(b'p', b'a'));
    }

    #[test]
    fn table_matches_the_function() {
        let table = super::adjacency_table();
        for a in b'a'..=b'z' {
            for b in b'a'..=b'z' {
                let bit = table[(a - b'a') as usize] & (1 << (b - b'a')) != 0;
                assert_eq!(bit, adjacent(a, b), "{} {}", a as char, b as char);
            }
        }
    }

    #[test]
    fn case_and_non_letters() {
        assert!(adjacent(b'Q', b'w'));
        assert!(!adjacent(b'a', b'a'));
        assert!(!adjacent(b'\'', b'l'));
    }
}
