// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::{ImeConfig, InputMethod, SmoothMode};
use crate::syllable::{
    analyze_syllable, find_tone_position, has_valid_vietnamese_initial,
    is_valid_vietnamese_syllable,
};
use crate::unicode::{Glyph, Tone, VowelMod};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyProcessResult {
    pub handled: bool,
    pub commit: Option<String>,
    pub preedit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LastAction {
    None,
    ToneApplied {
        prev_tone: Tone,
        trigger: char,
    },
    DStrokeApplied {
        index: usize,
        trigger: char,
    },
    VowelModApplied {
        indices: Vec<(usize, VowelMod)>,
        trigger: char,
    },
    StandaloneWHorn {
        index: usize,
        trigger: char,
    },
}

#[derive(Debug, Default, Clone)]
struct LockedMods {
    tone: bool,
    d_stroke: bool,
    circumflex_a: bool,
    circumflex_e: bool,
    circumflex_o: bool,
    horn_breve: bool,
}

pub struct ImeEngine {
    pub config: ImeConfig,
    glyphs: Vec<Glyph>,
    raw_keys: Vec<char>,
    tone: Tone,
    last_action: LastAction,
    locked: LockedMods,
    transformed: bool,
    capitalize_next: bool,
    last_committed_word_for_space: Option<String>,
}

impl ImeEngine {
    pub fn new(config: ImeConfig) -> Self {
        Self {
            config,
            glyphs: Vec::with_capacity(16),
            raw_keys: Vec::with_capacity(16),
            tone: Tone::None,
            last_action: LastAction::None,
            locked: LockedMods::default(),
            transformed: false,
            capitalize_next: false,
            last_committed_word_for_space: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    pub fn clear(&mut self) {
        self.glyphs.clear();
        self.raw_keys.clear();
        self.tone = Tone::None;
        self.last_action = LastAction::None;
        self.locked = LockedMods::default();
        self.transformed = false;
    }

    pub fn render_current_word(&self) -> String {
        if self.glyphs.is_empty() {
            return String::new();
        }
        let tone_idx = if self.tone != Tone::None {
            find_tone_position(&self.glyphs, self.config.modern_tone)
        } else {
            None
        };

        let mut out = String::with_capacity(self.glyphs.len() * 2);
        for (idx, g) in self.glyphs.iter().enumerate() {
            let applied_tone = if Some(idx) == tone_idx {
                self.tone
            } else {
                Tone::None
            };
            out.push(g.render(applied_tone));
        }
        out
    }

    pub fn render_raw_keys(&self) -> String {
        self.raw_keys.iter().collect()
    }

    pub fn finalize_word(&mut self) -> Option<String> {
        if self.glyphs.is_empty() {
            return None;
        }

        let rendered = self.render_current_word();
        let raw = self.render_raw_keys();

        let final_str = if let Some(expanded) = self.config.expand_macro(&rendered) {
            expanded
        } else if let Some(expanded_raw) = self.config.expand_macro(&raw) {
            expanded_raw
        } else if self.config.spell_check
            && self.transformed
            && !is_valid_vietnamese_syllable(
                &self.glyphs,
                self.tone,
                self.config.allow_foreign_consonants,
            )
        {
            raw
        } else {
            rendered
        };

        self.clear();
        Some(final_str)
    }

    pub fn process_backspace(&mut self) -> KeyProcessResult {
        self.last_committed_word_for_space = None;
        if self.glyphs.is_empty() {
            return KeyProcessResult {
                handled: false,
                commit: None,
                preedit: String::new(),
            };
        }

        self.glyphs.pop();
        self.raw_keys.pop();
        self.last_action = LastAction::None;

        if !self.glyphs.iter().any(|g| g.is_vowel()) {
            self.tone = Tone::None;
        }

        KeyProcessResult {
            handled: true,
            commit: None,
            preedit: self.render_current_word(),
        }
    }

    pub fn process_char(&mut self, mut ch: char) -> KeyProcessResult {
        if !self.config.enabled {
            return KeyProcessResult {
                handled: false,
                commit: None,
                preedit: String::new(),
            };
        }

        if ch == ' '
            && self.glyphs.is_empty()
            && self.config.double_space_period
            && self.last_committed_word_for_space.take().is_some()
        {
            self.capitalize_next = true;
            return KeyProcessResult {
                handled: true,
                commit: Some(". ".to_string()),
                preedit: String::new(),
            };
        }

        if self.config.method == InputMethod::Telex
            && self.config.bracket_shortcuts
            && matches!(ch, '[' | ']' | '{' | '}')
        {
            self.last_committed_word_for_space = None;
            return self.process_bracket_shortcut(ch);
        }

        if !self.is_word_char(ch) {
            if matches!(ch, '.' | '!' | '?') {
                self.capitalize_next = true;
            } else if ch != ' ' {
                self.capitalize_next = false;
            }

            if self.glyphs.is_empty() {
                self.last_committed_word_for_space = None;
                return KeyProcessResult {
                    handled: false,
                    commit: None,
                    preedit: String::new(),
                };
            }

            let committed_word = self.finalize_word().unwrap_or_default();
            if ch == ' ' && self.config.double_space_period {
                self.last_committed_word_for_space = Some(committed_word.clone());
            } else {
                self.last_committed_word_for_space = None;
            }

            let mut committed = committed_word;
            committed.push(ch);
            return KeyProcessResult {
                handled: true,
                commit: Some(committed),
                preedit: String::new(),
            };
        }

        self.last_committed_word_for_space = None;

        if self.glyphs.is_empty() && self.config.auto_capitalize && self.capitalize_next {
            ch = ch.to_ascii_uppercase();
            self.capitalize_next = false;
        }

        self.raw_keys.push(ch);

        if self.try_undo_double_key(ch) {
            return KeyProcessResult {
                handled: true,
                commit: None,
                preedit: self.render_current_word(),
            };
        }

        if self.config.quick_start_consonant {
            self.try_expand_quick_start_consonant(ch);
        }

        let allow_transforms = !self.config.spell_check
            || has_valid_vietnamese_initial(
                &self.glyphs,
                self.config.allow_foreign_consonants,
                self.config.quick_start_consonant,
            );

        if allow_transforms && self.try_apply_transformation(ch) {
            self.transformed = true;
            return KeyProcessResult {
                handled: true,
                commit: None,
                preedit: self.render_current_word(),
            };
        }

        if allow_transforms && self.config.quick_end_consonant && self.try_apply_quick_end(ch) {
            self.transformed = true;
            return KeyProcessResult {
                handled: true,
                commit: None,
                preedit: self.render_current_word(),
            };
        }

        self.glyphs.push(Glyph::new(ch));
        self.last_action = LastAction::None;

        if allow_transforms {
            self.normalize_uo_with_coda();
        }

        KeyProcessResult {
            handled: true,
            commit: None,
            preedit: self.render_current_word(),
        }
    }

    fn is_word_char(&self, ch: char) -> bool {
        if ch.is_ascii_alphabetic() {
            return true;
        }
        match self.config.method {
            InputMethod::Vni => ch.is_ascii_digit() && !self.glyphs.is_empty(),
            InputMethod::Viqr => {
                matches!(ch, '\'' | '`' | '?' | '~' | '.' | '^' | '+' | '*' | '(')
                    && !self.glyphs.is_empty()
            }
            _ => false,
        }
    }

    fn try_expand_quick_start_consonant(&mut self, next_ch: char) {
        let next_lower = next_ch.to_ascii_lowercase();
        if !matches!(next_lower, 'a' | 'e' | 'i' | 'o' | 'u' | 'y') || self.glyphs.len() != 1 {
            return;
        }

        let first = &self.glyphs[0];
        let is_upper = first.uppercase;

        if first.base == 'f' {
            let mut p = Glyph::new('p');
            p.uppercase = is_upper;
            let h = Glyph::new('h');
            self.glyphs = vec![p, h];
            self.transformed = true;
        } else if first.base == 'j' {
            let mut g = Glyph::new('g');
            g.uppercase = is_upper;
            let i = Glyph::new('i');
            self.glyphs = vec![g, i];
            self.transformed = true;
        } else if first.base == 'w'
            || (first.base == 'u'
                && first.vmod == VowelMod::Horn
                && self
                    .raw_keys
                    .first()
                    .is_some_and(|c| c.eq_ignore_ascii_case(&'w')))
        {
            let mut q = Glyph::new('q');
            q.uppercase = is_upper;
            let u = Glyph::new('u');
            self.glyphs = vec![q, u];
            self.last_action = LastAction::None;
            self.transformed = true;
        }
    }

    fn try_apply_quick_end(&mut self, ch: char) -> bool {
        let lower = ch.to_ascii_lowercase();
        if !matches!(lower, 'g' | 'h' | 'k') {
            return false;
        }
        let Some(last) = self.glyphs.last() else {
            return false;
        };
        if !last.is_vowel() {
            return false;
        }

        let is_upper = ch.is_ascii_uppercase();
        let (c1, c2) = match lower {
            'g' => ('n', 'g'),
            'h' => ('n', 'h'),
            'k' => ('c', 'h'),
            _ => return false,
        };

        let mut g1 = Glyph::new(c1);
        let mut g2 = Glyph::new(c2);
        g1.uppercase = is_upper;
        g2.uppercase = is_upper;
        self.glyphs.push(g1);
        self.glyphs.push(g2);
        self.last_action = LastAction::None;
        self.normalize_uo_with_coda();
        true
    }

    fn process_bracket_shortcut(&mut self, ch: char) -> KeyProcessResult {
        self.raw_keys.push(ch);
        let (base, uppercase) = match ch {
            '[' => ('o', false),
            '{' => ('o', true),
            ']' => ('u', false),
            '}' => ('u', true),
            _ => unreachable!(),
        };
        let mut g = Glyph::new(base);
        g.uppercase = uppercase;
        g.vmod = VowelMod::Horn;
        self.glyphs.push(g);
        self.transformed = true;
        self.last_action = LastAction::None;

        KeyProcessResult {
            handled: true,
            commit: None,
            preedit: self.render_current_word(),
        }
    }

    fn try_undo_double_key(&mut self, ch: char) -> bool {
        let lower = ch.to_ascii_lowercase();
        let action = self.last_action.clone();

        match action {
            LastAction::ToneApplied { prev_tone, trigger } if trigger == lower => {
                self.tone = prev_tone;
                self.glyphs.push(Glyph::new(ch));
                self.locked.tone = true;
                self.last_action = LastAction::None;
                self.sync_raw_keys_with_glyphs();
                true
            }
            LastAction::DStrokeApplied { index, trigger } if trigger == lower => {
                if let Some(g) = self.glyphs.get_mut(index) {
                    g.d_stroke = false;
                }
                self.glyphs.push(Glyph::new(ch));
                self.locked.d_stroke = true;
                self.last_action = LastAction::None;
                self.sync_raw_keys_with_glyphs();
                true
            }
            LastAction::VowelModApplied { indices, trigger } if trigger == lower => {
                for (idx, prev_mod) in indices {
                    if let Some(g) = self.glyphs.get_mut(idx) {
                        g.vmod = prev_mod;
                    }
                }
                self.glyphs.push(Glyph::new(ch));
                match lower {
                    'a' => self.locked.circumflex_a = true,
                    'e' => self.locked.circumflex_e = true,
                    'o' => self.locked.circumflex_o = true,
                    'w' | '7' | '8' | '+' | '*' | '(' => self.locked.horn_breve = true,
                    '6' | '^' => {
                        self.locked.circumflex_a = true;
                        self.locked.circumflex_e = true;
                        self.locked.circumflex_o = true;
                    }
                    _ => {}
                }
                self.last_action = LastAction::None;
                self.sync_raw_keys_with_glyphs();
                true
            }
            LastAction::StandaloneWHorn { index, trigger } if trigger == lower => {
                if let Some(g) = self.glyphs.get_mut(index) {
                    g.base = 'w';
                    g.vmod = VowelMod::None;
                }
                self.locked.horn_breve = true;
                self.last_action = LastAction::None;
                self.sync_raw_keys_with_glyphs();
                true
            }
            _ => false,
        }
    }

    fn sync_raw_keys_with_glyphs(&mut self) {
        if self.tone == Tone::None
            && self
                .glyphs
                .iter()
                .all(|g| g.vmod == VowelMod::None && !g.d_stroke)
        {
            self.raw_keys = self
                .glyphs
                .iter()
                .map(|g| {
                    if g.uppercase {
                        g.base.to_ascii_uppercase()
                    } else {
                        g.base
                    }
                })
                .collect();
            self.transformed = false;
        }
    }

    fn try_apply_transformation(&mut self, ch: char) -> bool {
        let lower = ch.to_ascii_lowercase();
        match self.config.method {
            InputMethod::Telex | InputMethod::SimpleTelex => {
                let allow_standalone_w = self.config.method == InputMethod::Telex
                    && (self.config.smooth_mode != SmoothMode::Gaming || !self.glyphs.is_empty());
                self.apply_telex(ch, lower, allow_standalone_w)
            }
            InputMethod::Vni => self.apply_vni(lower),
            InputMethod::Viqr => self.apply_viqr(lower),
        }
    }

    fn apply_telex(&mut self, orig_ch: char, lower: char, allow_standalone_w: bool) -> bool {
        match lower {
            's' => self.apply_tone(Tone::Acute, lower),
            'f' => self.apply_tone(Tone::Grave, lower),
            'r' => self.apply_tone(Tone::Hook, lower),
            'x' => self.apply_tone(Tone::Tilde, lower),
            'j' => self.apply_tone(Tone::Dot, lower),
            'z' => self.remove_tone(lower),
            'd' => self.apply_d_stroke(lower),
            'a' if !self.locked.circumflex_a => self.apply_circumflex('a', lower),
            'e' if !self.locked.circumflex_e => self.apply_circumflex('e', lower),
            'o' if !self.locked.circumflex_o => self.apply_circumflex('o', lower),
            'w' if !self.locked.horn_breve => {
                self.apply_w_modifier(orig_ch, lower, allow_standalone_w)
            }
            _ => false,
        }
    }

    fn apply_vni(&mut self, lower: char) -> bool {
        match lower {
            '1' => self.apply_tone(Tone::Acute, lower),
            '2' => self.apply_tone(Tone::Grave, lower),
            '3' => self.apply_tone(Tone::Hook, lower),
            '4' => self.apply_tone(Tone::Tilde, lower),
            '5' => self.apply_tone(Tone::Dot, lower),
            '0' => self.remove_tone(lower),
            '9' => self.apply_d_stroke(lower),
            '6' => {
                self.apply_circumflex('a', lower)
                    || self.apply_circumflex('e', lower)
                    || self.apply_circumflex('o', lower)
            }
            '7' if !self.locked.horn_breve => self.apply_vni_horn(lower),
            '8' if !self.locked.horn_breve => self.apply_breve_on_a(lower),
            _ => false,
        }
    }

    fn apply_viqr(&mut self, lower: char) -> bool {
        match lower {
            '\'' => self.apply_tone(Tone::Acute, lower),
            '`' => self.apply_tone(Tone::Grave, lower),
            '?' => self.apply_tone(Tone::Hook, lower),
            '~' => self.apply_tone(Tone::Tilde, lower),
            '.' => self.apply_tone(Tone::Dot, lower),
            'd' => self.apply_d_stroke(lower),
            '^' => {
                self.apply_circumflex('a', lower)
                    || self.apply_circumflex('e', lower)
                    || self.apply_circumflex('o', lower)
            }
            '+' | '*' if !self.locked.horn_breve => self.apply_vni_horn(lower),
            '(' if !self.locked.horn_breve => self.apply_breve_on_a(lower),
            _ => false,
        }
    }

    fn apply_tone(&mut self, new_tone: Tone, trigger: char) -> bool {
        if self.locked.tone {
            return false;
        }
        let analysis = analyze_syllable(&self.glyphs);
        if analysis.vowel_range.is_empty() {
            return false;
        }
        let prev_tone = self.tone;
        if prev_tone == new_tone {
            self.tone = Tone::None;
            self.glyphs.push(Glyph::new(trigger));
            self.locked.tone = true;
            self.last_action = LastAction::None;
            self.sync_raw_keys_with_glyphs();
            return true;
        }
        self.tone = new_tone;
        self.last_action = LastAction::ToneApplied { prev_tone, trigger };
        true
    }

    fn remove_tone(&mut self, trigger: char) -> bool {
        if self.tone == Tone::None {
            return false;
        }
        let prev_tone = self.tone;
        self.tone = Tone::None;
        self.last_action = LastAction::ToneApplied { prev_tone, trigger };
        true
    }

    fn apply_d_stroke(&mut self, trigger: char) -> bool {
        if self.locked.d_stroke || self.glyphs.is_empty() {
            return false;
        }
        let candidate_idx = if self.config.free_tone {
            if self.glyphs[0].base == 'd' && !self.glyphs[0].d_stroke {
                Some(0)
            } else {
                None
            }
        } else {
            let last = self.glyphs.len() - 1;
            if self.glyphs[last].base == 'd' && !self.glyphs[last].d_stroke {
                Some(last)
            } else {
                None
            }
        };

        if let Some(idx) = candidate_idx {
            self.glyphs[idx].d_stroke = true;
            self.last_action = LastAction::DStrokeApplied {
                index: idx,
                trigger,
            };
            true
        } else {
            false
        }
    }

    fn apply_circumflex(&mut self, target_base: char, trigger: char) -> bool {
        let analysis = analyze_syllable(&self.glyphs);
        if analysis.vowel_range.is_empty() {
            return false;
        }

        let search_range = if self.config.free_tone {
            analysis.vowel_range
        } else {
            let last = self.glyphs.len() - 1;
            last..self.glyphs.len()
        };

        for idx in search_range.rev() {
            if self.glyphs[idx].base == target_base {
                if self.glyphs[idx].vmod == VowelMod::Circumflex {
                    return false;
                }
                let prev = self.glyphs[idx].vmod;
                self.glyphs[idx].vmod = VowelMod::Circumflex;
                self.last_action = LastAction::VowelModApplied {
                    indices: vec![(idx, prev)],
                    trigger,
                };
                return true;
            }
        }
        false
    }

    fn apply_breve_on_a(&mut self, trigger: char) -> bool {
        let analysis = analyze_syllable(&self.glyphs);
        for idx in analysis.vowel_range.rev() {
            if self.glyphs[idx].base == 'a' && self.glyphs[idx].vmod != VowelMod::Breve {
                let prev = self.glyphs[idx].vmod;
                self.glyphs[idx].vmod = VowelMod::Breve;
                self.last_action = LastAction::VowelModApplied {
                    indices: vec![(idx, prev)],
                    trigger,
                };
                return true;
            }
        }
        false
    }

    fn apply_vni_horn(&mut self, trigger: char) -> bool {
        let analysis = analyze_syllable(&self.glyphs);
        let v_range = analysis.vowel_range;
        if v_range.is_empty() {
            return false;
        }

        let vowels = &self.glyphs[v_range.clone()];
        let has_coda = !analysis.final_range.is_empty();

        if vowels.len() >= 2 && vowels[0].base == 'u' && vowels[1].base == 'o' {
            let u_idx = v_range.start;
            let o_idx = v_range.start + 1;
            let prev_u = self.glyphs[u_idx].vmod;
            let prev_o = self.glyphs[o_idx].vmod;

            let is_huo_thuo =
                !has_coda && vowels.len() == 2 && self.is_h_or_th_initial(analysis.initial_range);
            if is_huo_thuo {
                self.glyphs[o_idx].vmod = VowelMod::Horn;
                self.last_action = LastAction::VowelModApplied {
                    indices: vec![(o_idx, prev_o)],
                    trigger,
                };
            } else {
                self.glyphs[u_idx].vmod = VowelMod::Horn;
                self.glyphs[o_idx].vmod = VowelMod::Horn;
                self.last_action = LastAction::VowelModApplied {
                    indices: vec![(u_idx, prev_u), (o_idx, prev_o)],
                    trigger,
                };
            }
            return true;
        }

        for idx in v_range.rev() {
            if matches!(self.glyphs[idx].base, 'o' | 'u')
                && self.glyphs[idx].vmod != VowelMod::Horn
            {
                let prev = self.glyphs[idx].vmod;
                self.glyphs[idx].vmod = VowelMod::Horn;
                self.last_action = LastAction::VowelModApplied {
                    indices: vec![(idx, prev)],
                    trigger,
                };
                return true;
            }
        }
        false
    }

    fn apply_w_modifier(&mut self, orig_ch: char, trigger: char, allow_standalone_w: bool) -> bool {
        let analysis = analyze_syllable(&self.glyphs);
        let v_range = analysis.vowel_range.clone();

        if !v_range.is_empty() {
            let vowels = &self.glyphs[v_range.clone()];
            let has_coda = !analysis.final_range.is_empty();

            if vowels.len() >= 2 && vowels[0].base == 'u' && vowels[1].base == 'o' {
                let u_idx = v_range.start;
                let o_idx = v_range.start + 1;
                let prev_u = self.glyphs[u_idx].vmod;
                let prev_o = self.glyphs[o_idx].vmod;

                if prev_u == VowelMod::Horn && prev_o == VowelMod::Horn {
                    return false;
                }

                let is_huo_thuo = !has_coda
                    && vowels.len() == 2
                    && prev_u == VowelMod::None
                    && self.is_h_or_th_initial(analysis.initial_range.clone());

                if is_huo_thuo {
                    self.glyphs[o_idx].vmod = VowelMod::Horn;
                    self.last_action = LastAction::VowelModApplied {
                        indices: vec![(o_idx, prev_o)],
                        trigger,
                    };
                } else {
                    self.glyphs[u_idx].vmod = VowelMod::Horn;
                    self.glyphs[o_idx].vmod = VowelMod::Horn;
                    let mut changed = Vec::new();
                    if prev_u != VowelMod::Horn {
                        changed.push((u_idx, prev_u));
                    }
                    if prev_o != VowelMod::Horn {
                        changed.push((o_idx, prev_o));
                    }
                    self.last_action = LastAction::VowelModApplied {
                        indices: changed,
                        trigger,
                    };
                }
                return true;
            }

            if vowels.len() == 2 && vowels[0].base == 'u' && vowels[1].base == 'a' {
                let target_idx = if has_coda {
                    v_range.start + 1
                } else {
                    v_range.start
                };
                let target_mod = if has_coda {
                    VowelMod::Breve
                } else {
                    VowelMod::Horn
                };
                if self.glyphs[target_idx].vmod != target_mod {
                    let prev = self.glyphs[target_idx].vmod;
                    self.glyphs[target_idx].vmod = target_mod;
                    self.last_action = LastAction::VowelModApplied {
                        indices: vec![(target_idx, prev)],
                        trigger,
                    };
                    return true;
                }
            }

            if vowels.len() == 2 && vowels[0].base == 'o' && vowels[1].base == 'a' {
                let a_idx = v_range.start + 1;
                if self.glyphs[a_idx].vmod != VowelMod::Breve {
                    let prev = self.glyphs[a_idx].vmod;
                    self.glyphs[a_idx].vmod = VowelMod::Breve;
                    self.last_action = LastAction::VowelModApplied {
                        indices: vec![(a_idx, prev)],
                        trigger,
                    };
                    return true;
                }
            }

            for idx in v_range.rev() {
                let base = self.glyphs[idx].base;
                let target_mod = match base {
                    'a' => Some(VowelMod::Breve),
                    'o' | 'u' => Some(VowelMod::Horn),
                    _ => None,
                };
                if let Some(m) = target_mod {
                    if self.glyphs[idx].vmod != m {
                        let prev = self.glyphs[idx].vmod;
                        self.glyphs[idx].vmod = m;
                        self.last_action = LastAction::VowelModApplied {
                            indices: vec![(idx, prev)],
                            trigger,
                        };
                        return true;
                    }
                }
            }
        }

        if allow_standalone_w && v_range.is_empty() && analysis.final_range.is_empty() {
            let is_after_q = self.glyphs.len() == 1 && self.glyphs[0].base == 'q';
            if !is_after_q {
                let mut g = Glyph::new('u');
                g.uppercase = orig_ch.is_ascii_uppercase();
                g.vmod = VowelMod::Horn;
                let new_idx = self.glyphs.len();
                self.glyphs.push(g);
                self.last_action = LastAction::StandaloneWHorn {
                    index: new_idx,
                    trigger,
                };
                return true;
            }
        }

        false
    }

    fn is_h_or_th_initial(&self, init_range: std::ops::Range<usize>) -> bool {
        let slice = &self.glyphs[init_range];
        (slice.len() == 1 && slice[0].base == 'h')
            || (slice.len() == 2 && slice[0].base == 't' && slice[1].base == 'h')
    }

    fn normalize_uo_with_coda(&mut self) {
        let analysis = analyze_syllable(&self.glyphs);
        let v_range = analysis.vowel_range;
        if v_range.len() < 2 {
            return;
        }
        let u_idx = v_range.start;
        let o_idx = v_range.start + 1;

        if self.glyphs[u_idx].base == 'u' && self.glyphs[o_idx].base == 'o' {
            let has_coda_or_offglide = !analysis.final_range.is_empty() || v_range.len() >= 3;
            if has_coda_or_offglide {
                let u_horn = self.glyphs[u_idx].vmod == VowelMod::Horn;
                let o_horn = self.glyphs[o_idx].vmod == VowelMod::Horn;
                if u_horn ^ o_horn {
                    self.glyphs[u_idx].vmod = VowelMod::Horn;
                    self.glyphs[o_idx].vmod = VowelMod::Horn;
                }
            }
        }
    }
}
