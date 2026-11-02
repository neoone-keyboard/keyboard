//! Malayalam → Latin, the way people type Manglish, and the "skeleton" that
//! collapses the spellings people don't type consistently. `lexbuild` keys
//! `manglish.fst` by the skeleton of each Malayalam word; the engine looks up
//! the skeleton of what was typed. Both call these functions.

const VIRAMA: char = '\u{0D4D}';

fn independent_vowel(c: char) -> Option<&'static str> {
    Some(match c {
        'അ' => "a",
        'ആ' => "aa",
        'ഇ' => "i",
        'ഈ' => "ee",
        'ഉ' => "u",
        'ഊ' => "oo",
        'ഋ' => "ru",
        'എ' => "e",
        'ഏ' => "e",
        'ഐ' => "ai",
        'ഒ' => "o",
        'ഓ' => "o",
        'ഔ' => "au",
        _ => return None,
    })
}

fn vowel_sign(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{0D3E}' => "aa", // ാ
        '\u{0D3F}' => "i",  // ി
        '\u{0D40}' => "ee", // ീ
        '\u{0D41}' => "u",  // ു
        '\u{0D42}' => "oo", // ൂ
        '\u{0D43}' => "ru", // ൃ
        '\u{0D46}' => "e",  // െ
        '\u{0D47}' => "e",  // േ
        '\u{0D48}' => "ai", // ൈ
        '\u{0D4A}' => "o",  // ൊ
        '\u{0D4B}' => "o",  // ോ
        '\u{0D4C}' => "au", // ൌ
        '\u{0D57}' => "au", // ൗ
        _ => return None,
    })
}

fn consonant(c: char) -> Option<&'static str> {
    Some(match c {
        'ക' => "k",
        'ഖ' => "kh",
        'ഗ' => "g",
        'ഘ' => "gh",
        'ങ' => "ng",
        'ച' => "ch",
        'ഛ' => "chh",
        'ജ' => "j",
        'ഝ' => "jh",
        'ഞ' => "nj",
        'ട' => "t",
        'ഠ' => "th",
        'ഡ' => "d",
        'ഢ' => "dh",
        'ണ' => "n",
        'ത' => "th",
        'ഥ' => "th",
        'ദ' => "d",
        'ധ' => "dh",
        'ന' => "n",
        'പ' => "p",
        'ഫ' => "ph",
        'ബ' => "b",
        'ഭ' => "bh",
        'മ' => "m",
        'യ' => "y",
        'ര' => "r",
        'റ' => "r",
        'ല' => "l",
        'ള' => "l",
        'ഴ' => "zh",
        'വ' => "v",
        'ശ' => "sh",
        'ഷ' => "sh",
        'സ' => "s",
        'ഹ' => "h",
        'ഺ' => "t",
        _ => return None,
    })
}

/// Chillus, anusvara and visarga: letters with no inherent vowel.
fn final_letter(c: char) -> Option<&'static str> {
    Some(match c {
        'ൺ' | 'ൻ' => "n",
        'ർ' => "r",
        'ൽ' | 'ൾ' => "l",
        'ൿ' => "k",
        'ം' => "m",
        'ഃ' => "h",
        _ => return None,
    })
}

/// Conjuncts typed as a unit: ന്റ is "nt" (ente), റ്റ is "tt", ങ്ങ "ng", ഞ്ഞ "nj", ക്ഷ "ksh".
fn cluster(chars: &[char]) -> Option<&'static str> {
    match chars {
        ['ന', VIRAMA, 'റ', ..] => Some("nt"),
        ['റ', VIRAMA, 'റ', ..] => Some("tt"),
        ['ങ', VIRAMA, 'ങ', ..] => Some("ng"),
        ['ഞ', VIRAMA, 'ഞ', ..] => Some("nj"),
        ['ക', VIRAMA, 'ഷ', ..] => Some("ksh"),
        _ => None,
    }
}

/// Romanises a (normalised) Malayalam word: എന്റെ → ente, വീട് → veetu,
/// മലയാളം → malayaalam. Non-Malayalam ASCII letters pass through lowercased.
pub fn romanize(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let mut out = String::with_capacity(word.len() * 2);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let consonant_latin = match cluster(&chars[i..]) {
            Some(latin) => {
                i += 3;
                Some(latin)
            }
            None => consonant(c).inspect(|_| i += 1),
        };
        if let Some(latin) = consonant_latin {
            out.push_str(latin);
            // The vowel this consonant carries.
            match chars.get(i) {
                Some(&next) if vowel_sign(next).is_some() => {
                    out.push_str(vowel_sign(next).unwrap_or_default());
                    i += 1;
                }
                Some(&VIRAMA) => {
                    i += 1;
                    // A word-final virama is the half-u people write as "u" (വീട് → veetu).
                    if i == chars.len() {
                        out.push('u');
                    }
                }
                _ => out.push('a'),
            }
            continue;
        }
        if let Some(latin) = independent_vowel(c).or_else(|| final_letter(c)) {
            out.push_str(latin);
        } else if c.is_ascii_alphabetic() {
            out.push(c.to_ascii_lowercase());
        }
        i += 1;
    }
    out
}

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Collapses Latin text to the distinctions people type consistently:
/// vowel length (aa/a, ee/i, oo/u), aspiration (th/t, dh/d, kh/gh/k …), voicing
/// of t/d, doubled letters, sh/s, and the trailing y, h, u or e people add or
/// drop at the end of a word. `ente`, `ende` and `entey` all give `ent`.
pub fn skeleton(latin: &str) -> String {
    let mut s: Vec<u8> = latin
        .bytes()
        .filter(u8::is_ascii_alphabetic)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    // entey → ente, ennah → enna
    if s.len() >= 3 && matches!(s[s.len() - 1], b'y' | b'h') && is_vowel(s[s.len() - 2]) {
        s.pop();
    }
    let mut out: Vec<u8> = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let next = s.get(i + 1).copied();
        let (symbols, used): (&[u8], usize) = match (s[i], next) {
            (b'c', Some(b'h')) if s.get(i + 2) == Some(&b'h') => (b"c", 3),
            (b'c', Some(b'h')) => (b"c", 2),
            (b'k', Some(b'h')) => (b"k", 2),
            // gh → k, not g: people write ഖ as "gh" as often as "kh" (sugham,
            // mugham for സുഖം, മുഖം), and ഖ is far commoner than ഘ. Plain g
            // and k stay apart (kada / gada).
            (b'g', Some(b'h')) => (b"k", 2),
            (b'j', Some(b'h')) => (b"j", 2),
            (b't' | b'd', Some(b'h')) => (b"t", 2),
            (b'p', Some(b'h')) => (b"p", 2),
            (b'b', Some(b'h')) => (b"b", 2),
            (b's', Some(b'h')) => (b"s", 2),
            (b'z', Some(b'h')) => (b"z", 2),
            (b'a', Some(b'a')) => (b"a", 2),
            (b'e', Some(b'e')) | (b'i', Some(b'i')) => (b"i", 2),
            (b'o', Some(b'o')) | (b'u', Some(b'u')) => (b"u", 2),
            (b, _) => (single(b), 1),
        };
        for &sym in symbols {
            if out.last() != Some(&sym) {
                out.push(sym);
            }
        }
        i += used;
    }
    // veedu → veed, ente → ent: the final vowel after a consonant is often dropped.
    if out.len() >= 3 && matches!(out[out.len() - 1], b'u' | b'e') && !is_vowel(out[out.len() - 2])
    {
        out.pop();
    }
    String::from_utf8(out).unwrap_or_default()
}

/// One Latin letter's skeleton symbol(s).
fn single(b: u8) -> &'static [u8] {
    match b {
        b'c' | b'q' => b"k",
        b'd' => b"t",
        b'f' => b"p",
        b'w' => b"v",
        b'x' => b"ks",
        b'a' => b"a",
        b'b' => b"b",
        b'e' => b"e",
        b'g' => b"g",
        b'h' => b"h",
        b'i' => b"i",
        b'j' => b"j",
        b'k' => b"k",
        b'l' => b"l",
        b'm' => b"m",
        b'n' => b"n",
        b'o' => b"o",
        b'p' => b"p",
        b'r' => b"r",
        b's' => b"s",
        b't' => b"t",
        b'u' => b"u",
        b'v' => b"v",
        b'y' => b"y",
        b'z' => b"z",
        _ => b"",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn romanises_common_words() {
        assert_eq!(romanize("എന്റെ"), "ente");
        assert_eq!(romanize("നിന്റെ"), "ninte");
        assert_eq!(romanize("വീട്"), "veetu");
        assert_eq!(romanize("മലയാളം"), "malayaalam");
        assert_eq!(romanize("ഞാൻ"), "njaan");
        assert_eq!(romanize("അമ്മ"), "amma");
        assert_eq!(romanize("എനിക്ക്"), "enikku");
        assert_eq!(romanize("കേരളം"), "keralam");
        assert_eq!(romanize("സുഖമാണോ"), "sukhamaano");
        assert_eq!(romanize("കുട്ടി"), "kutti");
        assert_eq!(romanize("ഇല്ല"), "illa");
        assert_eq!(romanize("പോയി"), "poyi");
        assert_eq!(romanize("ചോറ്റ്"), "chottu");
        assert_eq!(romanize("ഭക്ഷണം"), "bhakshanam");
    }

    #[test]
    fn spellings_people_type_share_a_skeleton() {
        let ente = skeleton(&romanize("എന്റെ"));
        assert_eq!(ente, "ent");
        for typed in ["ente", "ende", "entey", "enthe", "Ente"] {
            assert_eq!(skeleton(typed), ente, "{typed}");
        }
        let veedu = skeleton(&romanize("വീട്"));
        for typed in ["veedu", "veed", "vidu", "veetu"] {
            assert_eq!(skeleton(typed), veedu, "{typed}");
        }
        let njan = skeleton(&romanize("ഞാൻ"));
        assert_eq!(skeleton("njan"), njan);
        assert_eq!(skeleton("njaan"), njan);
        assert_ne!(
            skeleton("njn"),
            njan,
            "vowels are never dropped from the middle"
        );
        let malayalam = skeleton(&romanize("മലയാളം"));
        assert_eq!(skeleton("malayalam"), malayalam);
        assert_eq!(skeleton("malayaalam"), malayalam);
        let enikku = skeleton(&romanize("എനിക്ക്"));
        for typed in ["enikku", "eniku", "enikk", "enik"] {
            assert_eq!(skeleton(typed), enikku, "{typed}");
        }
        assert_eq!(skeleton("sukhamano"), skeleton(&romanize("സുഖമാണോ")));
        assert_eq!(skeleton("bhakshanam"), skeleton("bakshanam"));
    }

    /// Asserts every typed spelling collapses to the skeleton of `malayalam`.
    fn assert_typed_as(malayalam: &str, typed: &[&str]) {
        let want = skeleton(&romanize(malayalam));
        for t in typed {
            assert_eq!(skeleton(t), want, "{t} → {malayalam}");
        }
    }

    #[test]
    fn common_manglish_spellings_reach_their_word() {
        assert_typed_as("എന്റെ", &["ente", "ende", "entey"]);
        assert_typed_as("ഞാൻ", &["njan", "njaan"]);
        assert_typed_as("നിങ്ങൾ", &["ningal", "ningale"]);
        assert_typed_as("സുഖം", &["sukham", "sugham", "sukam"]);
        assert_typed_as("വീട്", &["veedu", "vidu"]);
        assert_typed_as("കൊള്ളാം", &["kollam", "kolam"]);
        assert_typed_as("പിന്നെ", &["pinne", "pine"]);
        assert_typed_as("എന്ത്", &["enthu", "enth"]);
        assert_typed_as("അമ്മയെ", &["ammaye", "amaye"]);
        // ഘ shares the collapsed k with ഖ, so its own spellings still agree.
        assert_typed_as("മേഘം", &["megham", "mekham"]);
        // The cost: ഘ typed as a bare "g" no longer matches.
        assert_ne!(skeleton("megam"), skeleton(&romanize("മേഘം")));
    }

    #[test]
    fn final_a_is_a_real_vowel() {
        // Decision: "entha" is എന്താ, not എന്ത്. A final -a is never dropped,
        // because it tells words apart (എന്താ / എന്ത്, അമ്മ / അമ്മു). എന്ത് is
        // still reached from "entha" by the prefix lookup of "enth", and both
        // forms share the skeleton prefix "ent".
        assert_typed_as("എന്താ", &["entha", "enthaa", "enta"]);
        assert_ne!(skeleton("entha"), skeleton(&romanize("എന്ത്")));
        assert!(skeleton("entha").starts_with(&skeleton(&romanize("എന്ത്"))));
    }

    #[test]
    fn different_words_keep_different_skeletons() {
        assert_ne!(skeleton("amma"), skeleton("ammu"));
        assert_ne!(skeleton("pani"), skeleton("bani"));
        assert_ne!(skeleton("kada"), skeleton("gada"));
    }

    #[test]
    fn skeleton_ignores_case_and_non_letters() {
        assert_eq!(skeleton("Ente!"), "ent");
        assert_eq!(skeleton(""), "");
        assert_eq!(skeleton("123"), "");
    }

    proptest! {
        #[test]
        fn never_panics_and_is_lowercase_ascii(s in "\\PC{0,24}") {
            let r = romanize(&s);
            let k = skeleton(&r);
            prop_assert!(k.bytes().all(|b| b.is_ascii_lowercase()));
            let _ = skeleton(&s);
        }

        #[test]
        fn skeleton_ignores_case(s in "[a-zA-Z]{0,16}") {
            prop_assert_eq!(skeleton(&s), skeleton(&s.to_ascii_lowercase()));
        }
    }
}
