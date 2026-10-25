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
enum ActionRecord {
    Tone { prev: Tone },
    DStroke { idx: usize },
    VowelMod { changes: Vec<(usize, VowelMod)> },
    AppendGlyph,
    StandaloneW { idx: usize },
}

pub struct ImeEngine {
    pub config: ImeConfig,
    glyphs: Vec<Glyph>,
    raw_keys: Vec<char>,
    history: Vec<ActionRecord>,
    tone: Tone,
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
            history: Vec::with_capacity(16),
            tone: Tone::None,
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
        self.history.clear();
        self.tone = Tone::None;
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
            out.push_str(&g.render_with_charset(applied_tone, self.config.charset));
        }
        out
    }

    /// Hoàn tác từng bước chính xác khi nhấn BackSpace
    pub fn process_backspace(&mut self) -> KeyProcessResult {
        self.last_committed_word_for_space = None;
        if self.glyphs.is_empty() {
            self.raw_keys.clear();
            self.history.clear();
            return KeyProcessResult {
                handled: false,
                commit: None,
                preedit: String::new(),
            };
        }

        self.raw_keys.pop();
        if let Some(last_action) = self.history.pop() {
            match last_action {
                ActionRecord::Tone { prev } => {
                    self.tone = prev;
                }
                ActionRecord::DStroke { idx } => {
                    if let Some(g) = self.glyphs.get_mut(idx) {
                        g.d_stroke = false;
                    }
                }
                ActionRecord::VowelMod { changes } => {
                    for (idx, prev_mod) in changes {
                        if let Some(g) = self.glyphs.get_mut(idx) {
                            g.vmod = prev_mod;
                        }
                    }
                }
                ActionRecord::AppendGlyph | ActionRecord::StandaloneW { .. } => {
                    self.glyphs.pop();
                }
            }
        } else {
            self.glyphs.pop();
        }

        if !self.glyphs.iter().any(|g| g.is_vowel()) {
            self.tone = Tone::None;
        }

        KeyProcessResult {
            handled: true,
            commit: None,
            preedit: self.render_current_word(),
        }
    }

    /// Xử lý phím W độc lập mà không làm nuốt ký tự khi gõ "www"
    fn apply_standalone_w(&mut self, orig_ch: char, allow_standalone: bool) -> bool {
        if !allow_standalone {
            return false;
        }

        let analysis = analyze_syllable(&self.glyphs);
        if analysis.vowel_range.is_empty() && analysis.final_range.is_empty() {
            let mut g = Glyph::new('u');
            g.uppercase = orig_ch.is_ascii_uppercase();
            g.vmod = VowelMod::Horn;
            let idx = self.glyphs.len();
            self.glyphs.push(g);
            self.history.push(ActionRecord::StandaloneW { idx });
            return true;
        }
        false
    }

    /// Phục hồi chính xác khi nhấn lặp phím W: "ư" -> "ww"
    pub fn try_undo_w_duplicate(&mut self, ch: char) -> bool {
        if ch.to_ascii_lowercase() != 'w' {
            return false;
        }
        if let Some(ActionRecord::StandaloneW { idx }) = self.history.last() {
            let idx = *idx;
            if let Some(g) = self.glyphs.get_mut(idx) {
                g.base = 'w';
                g.vmod = VowelMod::None;
            }
            self.history.pop();
            // Thêm ký tự W thứ hai vào danh sách thay vì chỉ đổi tại chỗ
            self.glyphs.push(Glyph::new(ch));
            self.history.push(ActionRecord::AppendGlyph);
            return true;
        }
        false
    }
}
