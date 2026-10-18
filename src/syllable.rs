// SPDX-License-Identifier: GPL-3.0-or-later

use crate::unicode::{Glyph, Tone, VowelMod};

#[derive(Debug, Clone)]
pub struct SyllableAnalysis {
    pub initial_range: std::ops::Range<usize>,
    pub vowel_range: std::ops::Range<usize>,
    pub final_range: std::ops::Range<usize>,
    pub is_structure_valid: bool,
}

const VALID_INITIALS: &[&str] = &[
    "", "b", "c", "ch", "d", "đ", "g", "gh", "gi", "h", "k", "kh", "l", "m", "n", "ng", "ngh",
    "nh", "p", "ph", "qu", "r", "s", "t", "th", "tr", "v", "x",
];

const FOREIGN_INITIALS: &[&str] = &[
    "f", "j", "w", "z", "br", "cl", "cr", "dr", "fl", "fr", "gl", "gr", "pl", "pr", "sc", "sh",
    "sl", "sm", "sp", "st", "str", "sw", "tr",
];

const VALID_FINALS: &[&str] = &["", "c", "ch", "m", "n", "ng", "nh", "p", "t"];

const OPEN_VOWEL_CLUSTERS: &[&str] = &[
    "a", "e", "ê", "i", "o", "ô", "ơ", "u", "ư", "y", "ai", "ao", "au", "ay", "âu", "ây", "eo",
    "êu", "ia", "iu", "oa", "oe", "oi", "ôi", "ơi", "ua", "ui", "ưi", "ưa", "ưu", "uơ", "ươ", "uy",
    "uê", "oo", "iêu", "yêu", "oai", "oay", "oao", "uôi", "ươi", "ươu", "uya", "uyu", "uây",
];

const CLOSED_VOWEL_CLUSTERS: &[&str] = &[
    "a", "ă", "â", "e", "ê", "i", "o", "ô", "ơ", "u", "ư", "y", "iê", "yê", "ia", "ua", "uâ", "uô",
    "ươ", "oa", "oă", "oe", "uê", "uy", "oo", "uyê",
];

pub fn analyze_syllable(glyphs: &[Glyph]) -> SyllableAnalysis {
    let len = glyphs.len();
    if len == 0 {
        return SyllableAnalysis {
            initial_range: 0..0,
            vowel_range: 0..0,
            final_range: 0..0,
            is_structure_valid: true,
        };
    }

    let mut v_start = None;
    for (i, g) in glyphs.iter().enumerate() {
        if g.is_vowel() {
            v_start = Some(i);
            break;
        }
    }

    let Some(mut v_start) = v_start else {
        return SyllableAnalysis {
            initial_range: 0..len,
            vowel_range: len..len,
            final_range: len..len,
            is_structure_valid: true,
        };
    };

    if len > 2
        && v_start == 1
        && glyphs[0].base == 'q'
        && glyphs[1].base == 'u'
        && glyphs[1].vmod == VowelMod::None
        && glyphs[2].is_vowel()
    {
        v_start = 2;
    }

    if len > 2
        && v_start == 1
        && glyphs[0].base == 'g'
        && glyphs[1].base == 'i'
        && glyphs[1].vmod == VowelMod::None
        && glyphs[2].is_vowel()
    {
        v_start = 2;
    }

    let mut v_end = v_start;
    while v_end < len && glyphs[v_end].is_vowel() {
        v_end += 1;
    }

    let has_trailing_vowels = glyphs[v_end..].iter().any(|g| g.is_vowel());

    SyllableAnalysis {
        initial_range: 0..v_start,
        vowel_range: v_start..v_end,
        final_range: v_end..len,
        is_structure_valid: !has_trailing_vowels,
    }
}

pub fn has_valid_vietnamese_initial(
    glyphs: &[Glyph],
    allow_foreign: bool,
    quick_start: bool,
) -> bool {
    let analysis = analyze_syllable(glyphs);
    let init_slice = &glyphs[analysis.initial_range];

    let mut init_str = String::with_capacity(4);
    for g in init_slice {
        if g.base == 'd' && g.d_stroke {
            init_str.push('đ');
        } else {
            init_str.push(g.base);
        }
    }

    if init_str == "q" || init_str == "dd" {
        return true;
    }

    if quick_start && matches!(init_str.as_str(), "f" | "j" | "w") {
        return true;
    }

    if allow_foreign && FOREIGN_INITIALS.contains(&init_str.as_str()) {
        return true;
    }

    VALID_INITIALS.contains(&init_str.as_str())
}

pub fn find_tone_position(glyphs: &[Glyph], modern_tone: bool) -> Option<usize> {
    let analysis = analyze_syllable(glyphs);
    if analysis.vowel_range.is_empty() {
        return None;
    }

    let v_start = analysis.vowel_range.start;
    let has_coda = !analysis.final_range.is_empty();
    let vowels = &glyphs[analysis.vowel_range];

    if vowels.len() == 1 {
        return Some(v_start);
    }

    let modded_positions: Vec<usize> = vowels
        .iter()
        .enumerate()
        .filter_map(|(idx, g)| {
            if g.vmod != VowelMod::None {
                Some(v_start + idx)
            } else {
                None
            }
        })
        .collect();

    if modded_positions.len() == 1 {
        return Some(modded_positions[0]);
    } else if modded_positions.len() >= 2 {
        return Some(modded_positions[1]);
    }

    if vowels.len() == 2 {
        if has_coda {
            return Some(v_start + 1);
        }
        let pair = (vowels[0].base, vowels[1].base);
        if matches!(pair, ('o', 'a') | ('o', 'e') | ('u', 'y')) {
            return if modern_tone {
                Some(v_start + 1)
            } else {
                Some(v_start)
            };
        }
        return Some(v_start);
    }

    Some(v_start + 1)
}

pub fn is_valid_vietnamese_syllable(glyphs: &[Glyph], tone: Tone, allow_foreign: bool) -> bool {
    if glyphs.is_empty() {
        return false;
    }

    let analysis = analyze_syllable(glyphs);
    if !analysis.is_structure_valid || analysis.vowel_range.is_empty() {
        return false;
    }

    let mut initial = String::with_capacity(4);
    for g in &glyphs[analysis.initial_range] {
        if g.base == 'd' && g.d_stroke {
            initial.push('đ');
        } else {
            initial.push(g.base);
        }
    }
    let valid_init = VALID_INITIALS.contains(&initial.as_str())
        || (allow_foreign && FOREIGN_INITIALS.contains(&initial.as_str()));
    if !valid_init {
        return false;
    }

    let mut final_cons = String::with_capacity(4);
    for g in &glyphs[analysis.final_range] {
        if g.d_stroke {
            return false;
        }
        final_cons.push(g.base);
    }
    if !VALID_FINALS.contains(&final_cons.as_str()) {
        return false;
    }

    if matches!(final_cons.as_str(), "c" | "ch" | "p" | "t")
        && !matches!(tone, Tone::Acute | Tone::Dot)
    {
        return false;
    }

    let mut vowel_cluster = String::with_capacity(6);
    for g in &glyphs[analysis.vowel_range] {
        vowel_cluster.push(g.modded_vowel_char());
    }

    if final_cons.is_empty() {
        OPEN_VOWEL_CLUSTERS.contains(&vowel_cluster.as_str())
    } else {
        CLOSED_VOWEL_CLUSTERS.contains(&vowel_cluster.as_str())
    }
}
