//! Latin pronunciation for lyrics in Asian scripts, shown under each line:
//! Japanese → romaji (Hepburn; kanji readings from the IPADIC dictionary via
//! lindera), Chinese → pinyin with tone marks, Korean → Revised Romanization.
//! All offline; libraries are MIT-licensed (kakasi was ruled out: GPL).

use std::borrow::Cow;
use std::sync::OnceLock;

use lindera::dictionary::load_dictionary;
use lindera::mode::Mode;
use lindera::segmenter::Segmenter;

fn is_kana(c: char) -> bool {
    ('\u{3040}'..='\u{30ff}').contains(&c) || ('\u{31f0}'..='\u{31ff}').contains(&c) || c == 'ー'
}
fn is_han(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c) || ('\u{f900}'..='\u{faff}').contains(&c)
}
fn is_hangul(c: char) -> bool {
    ('\u{ac00}'..='\u{d7a3}').contains(&c) || ('\u{1100}'..='\u{11ff}').contains(&c) || ('\u{3130}'..='\u{318f}').contains(&c)
}

#[derive(Clone, Copy, PartialEq)]
enum Script {
    Japanese,
    Chinese,
    Korean,
}

/// Romanize every line. `None` when the text has nothing to romanize; inside,
/// `None` for lines that are already Latin.
pub fn lines(lines: &[String]) -> Option<Vec<Option<String>>> {
    let all: String = lines.concat();
    let script = if all.chars().any(is_kana) {
        Script::Japanese
    } else if all.chars().any(is_hangul) {
        Script::Korean
    } else if all.chars().any(is_han) {
        Script::Chinese
    } else {
        return None;
    };
    Some(
        lines
            .iter()
            .map(|l| {
                if !l.chars().any(|c| is_kana(c) || is_han(c) || is_hangul(c)) {
                    return None;
                }
                let r = match script {
                    Script::Japanese => japanese(l),
                    Script::Chinese => chinese(l),
                    Script::Korean => korean(l),
                };
                let r = r.split_whitespace().collect::<Vec<_>>().join(" ");
                (!r.is_empty() && r != *l).then_some(r)
            })
            .collect(),
    )
}

// ------------------------------------------------------------------ Chinese

fn chinese(line: &str) -> String {
    use pinyin::ToPinyin;
    let mut out = String::new();
    for c in line.chars() {
        match c.to_pinyin() {
            Some(p) => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(p.with_tone());
                out.push(' ');
            }
            None => out.push(c),
        }
    }
    out
}

// ------------------------------------------------------------------ Korean

fn korean(line: &str) -> String {
    const INITIAL: [&str; 19] = ["g", "kk", "n", "d", "tt", "r", "m", "b", "pp", "s", "ss", "", "j", "jj", "ch", "k", "t", "p", "h"];
    const VOWEL: [&str; 21] = ["a", "ae", "ya", "yae", "eo", "e", "yeo", "ye", "o", "wa", "wae", "oe", "yo", "u", "wo", "we", "wi", "yu", "eu", "ui", "i"];
    const FINAL: [&str; 28] = ["", "k", "k", "k", "n", "n", "n", "t", "l", "k", "m", "p", "l", "l", "p", "l", "m", "p", "p", "t", "t", "ng", "t", "t", "k", "t", "p", "t"];
    let mut out = String::new();
    for c in line.chars() {
        let u = c as u32;
        if (0xac00..=0xd7a3).contains(&u) {
            let s = u - 0xac00;
            out.push_str(INITIAL[(s / 588) as usize]);
            out.push_str(VOWEL[((s % 588) / 28) as usize]);
            out.push_str(FINAL[(s % 28) as usize]);
        } else {
            out.push(c);
        }
    }
    out
}

// ------------------------------------------------------------------ Japanese

fn segmenter() -> Option<&'static Segmenter> {
    static SEG: OnceLock<Option<Segmenter>> = OnceLock::new();
    SEG.get_or_init(|| {
        let dict = load_dictionary("embedded://ipadic").map_err(|e| log::warn!("romaji dictionary: {e}")).ok()?;
        Some(Segmenter::new(Mode::Normal, dict, None))
    })
    .as_ref()
}

fn japanese(line: &str) -> String {
    let Some(seg) = segmenter() else { return kana_to_romaji(line) };
    let Ok(mut tokens) = seg.segment(Cow::Borrowed(line)) else { return kana_to_romaji(line) };
    let mut words: Vec<String> = Vec::new();
    for t in tokens.iter_mut() {
        let surface = t.surface.to_string();
        let details: Vec<String> = t.details().iter().map(|d| d.to_string()).collect();
        let particle = details.first().is_some_and(|p| p == "助詞");
        // IPADIC: index 7 is the reading (katakana); "*" or missing when unknown.
        let reading = details.get(7).filter(|r| r.as_str() != "*" && !r.is_empty()).cloned();
        let word = match (particle, surface.as_str()) {
            (true, "は") => "wa".to_string(),
            (true, "へ") => "e".to_string(),
            (true, "を") => "o".to_string(),
            _ => match reading {
                Some(r) => kana_to_romaji(&r),
                None => kana_to_romaji(&surface),
            },
        };
        if word.trim().is_empty() {
            continue;
        }
        // Glue punctuation to the previous word.
        if word.chars().all(|c| !c.is_alphanumeric()) {
            if let Some(last) = words.last_mut() {
                last.push_str(&word);
                continue;
            }
        }
        words.push(word);
    }
    words.join(" ")
}

/// Hepburn romaji for hiragana / katakana; other characters pass through.
pub fn kana_to_romaji(text: &str) -> String {
    // Katakana -> hiragana, so one table serves both.
    let hira: Vec<char> = text
        .chars()
        .map(|c| if ('\u{30a1}'..='\u{30f6}').contains(&c) { char::from_u32(c as u32 - 0x60).unwrap_or(c) } else { c })
        .collect();
    let mut out = String::new();
    let mut i = 0;
    let mut double_next = false;
    while i < hira.len() {
        let c = hira[i];
        // Two-character combinations (きゃ, しゅ, ちょ...).
        if i + 1 < hira.len() {
            let pair: String = [c, hira[i + 1]].iter().collect();
            if let Some(r) = combo(&pair) {
                push_syllable(&mut out, r, &mut double_next);
                i += 2;
                continue;
            }
        }
        match c {
            'っ' => double_next = true,
            'ー' => {
                if let Some(v) = out.chars().last().filter(|v| "aeiou".contains(*v)) {
                    out.push(v);
                }
            }
            _ => match single(c) {
                Some(r) => push_syllable(&mut out, r, &mut double_next),
                None => {
                    double_next = false;
                    out.push(match c {
                        '、' => ',',
                        '。' => '.',
                        '「' | '」' | '『' | '』' => '"',
                        '！' => '!',
                        '？' => '?',
                        '　' => ' ',
                        _ => c,
                    });
                }
            },
        }
        i += 1;
    }
    out
}

fn push_syllable(out: &mut String, r: &str, double_next: &mut bool) {
    if std::mem::take(double_next) {
        match r.chars().next() {
            Some('c') => out.push('t'),
            Some(c) if !"aeiou".contains(c) => out.push(c),
            _ => {}
        }
    }
    out.push_str(r);
}

fn combo(p: &str) -> Option<&'static str> {
    Some(match p {
        "きゃ" => "kya", "きゅ" => "kyu", "きょ" => "kyo",
        "ぎゃ" => "gya", "ぎゅ" => "gyu", "ぎょ" => "gyo",
        "しゃ" => "sha", "しゅ" => "shu", "しょ" => "sho", "しぇ" => "she",
        "じゃ" => "ja", "じゅ" => "ju", "じょ" => "jo", "じぇ" => "je",
        "ちゃ" => "cha", "ちゅ" => "chu", "ちょ" => "cho", "ちぇ" => "che",
        "にゃ" => "nya", "にゅ" => "nyu", "にょ" => "nyo",
        "ひゃ" => "hya", "ひゅ" => "hyu", "ひょ" => "hyo",
        "びゃ" => "bya", "びゅ" => "byu", "びょ" => "byo",
        "ぴゃ" => "pya", "ぴゅ" => "pyu", "ぴょ" => "pyo",
        "みゃ" => "mya", "みゅ" => "myu", "みょ" => "myo",
        "りゃ" => "rya", "りゅ" => "ryu", "りょ" => "ryo",
        "ふぁ" => "fa", "ふぃ" => "fi", "ふぇ" => "fe", "ふぉ" => "fo",
        "てぃ" => "ti", "でぃ" => "di", "とぅ" => "tu", "どぅ" => "du",
        "うぃ" => "wi", "うぇ" => "we", "うぉ" => "wo",
        "ゔぁ" => "va", "ゔぃ" => "vi", "ゔぇ" => "ve", "ゔぉ" => "vo",
        "つぁ" => "tsa", "つぇ" => "tse", "つぉ" => "tso",
        _ => return None,
    })
}

fn single(c: char) -> Option<&'static str> {
    Some(match c {
        'あ' => "a", 'い' => "i", 'う' => "u", 'え' => "e", 'お' => "o",
        'ぁ' => "a", 'ぃ' => "i", 'ぅ' => "u", 'ぇ' => "e", 'ぉ' => "o",
        'か' => "ka", 'き' => "ki", 'く' => "ku", 'け' => "ke", 'こ' => "ko",
        'が' => "ga", 'ぎ' => "gi", 'ぐ' => "gu", 'げ' => "ge", 'ご' => "go",
        'さ' => "sa", 'し' => "shi", 'す' => "su", 'せ' => "se", 'そ' => "so",
        'ざ' => "za", 'じ' => "ji", 'ず' => "zu", 'ぜ' => "ze", 'ぞ' => "zo",
        'た' => "ta", 'ち' => "chi", 'つ' => "tsu", 'て' => "te", 'と' => "to",
        'だ' => "da", 'ぢ' => "ji", 'づ' => "zu", 'で' => "de", 'ど' => "do",
        'な' => "na", 'に' => "ni", 'ぬ' => "nu", 'ね' => "ne", 'の' => "no",
        'は' => "ha", 'ひ' => "hi", 'ふ' => "fu", 'へ' => "he", 'ほ' => "ho",
        'ば' => "ba", 'び' => "bi", 'ぶ' => "bu", 'べ' => "be", 'ぼ' => "bo",
        'ぱ' => "pa", 'ぴ' => "pi", 'ぷ' => "pu", 'ぺ' => "pe", 'ぽ' => "po",
        'ま' => "ma", 'み' => "mi", 'む' => "mu", 'め' => "me", 'も' => "mo",
        'や' => "ya", 'ゆ' => "yu", 'よ' => "yo", 'ゃ' => "ya", 'ゅ' => "yu", 'ょ' => "yo",
        'ら' => "ra", 'り' => "ri", 'る' => "ru", 'れ' => "re", 'ろ' => "ro",
        'わ' => "wa", 'ゐ' => "wi", 'ゑ' => "we", 'を' => "o", 'ん' => "n", 'ゔ' => "vu",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kana() {
        assert_eq!(kana_to_romaji("きょうは"), "kyouha");
        assert_eq!(kana_to_romaji("がっこう"), "gakkou");
        assert_eq!(kana_to_romaji("コーヒー"), "koohii");
        assert_eq!(kana_to_romaji("まっちゃ"), "matcha");
    }

    #[test]
    fn korean_and_chinese() {
        assert_eq!(korean("안녕"), "annyeong");
        assert_eq!(chinese("你好").split_whitespace().collect::<Vec<_>>(), vec!["nǐ", "hǎo"]);
    }

    #[test]
    fn japanese_kanji() {
        let r = lines(&["私は猫です".to_string()]).unwrap();
        assert_eq!(r[0].as_deref(), Some("watashi wa neko desu"));
    }
}
