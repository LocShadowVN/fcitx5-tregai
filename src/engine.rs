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

        self.glyphs.pus
