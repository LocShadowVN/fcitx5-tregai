// SPDX-License-Identifier: GPL-3.0-or-later

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VowelMod {
    None,
    Circumflex, // ^ (â, ê, ô)
    Breve,      // ă
    Horn,       // ơ, ư
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Tone {
    None = 0,  // Ngang
    Acute = 1, // Sắc
    Grave = 2, // Huyền
    Hook = 3,  // Hỏi
    Tilde = 4, // Ngã
    Dot = 5,   // Nặng
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glyph {
    pub base: char,
    pub uppercase: bool,
    pub vmod: VowelMod,
    pub d_stroke: bool,
}

impl Glyph {
    pub fn new(ch: char) -> Self {
        Self {
            base: ch.to_ascii_lowercase(),
            uppercase: ch.is_ascii_uppercase(),
            vmod: VowelMod::None,
            d_stroke: false,
        }
    }

    pub fn is_vowel(&self) -> bool {
        matches!(self.base, 'a' | 'e' | 'i' | 'o' | 'u' | 'y')
    }

    pub fn render(&self, tone: Tone) -> char {
        if self.base == 'd' {
            return match (self.d_stroke, self.uppercase) {
                (true, false) => 'đ',
                (true, true) => 'Đ',
                (false, false) => 'd',
                (false, true) => 'D',
            };
        }

        let t = tone as usize;
        let ch = match (self.base, self.vmod) {
            ('a', VowelMod::None) => ['a', 'á', 'à', 'ả', 'ã', 'ạ'][t],
            ('a', VowelMod::Breve) => ['ă', 'ắ', 'ằ', 'ẳ', 'ẵ', 'ặ'][t],
            ('a', VowelMod::Circumflex) => ['â', 'ấ', 'ầ', 'ẩ', 'ẫ', 'ậ'][t],
            ('e', VowelMod::None) => ['e', 'é', 'è', 'ẻ', 'ẽ', 'ẹ'][t],
            ('e', VowelMod::Circumflex) => ['ê', 'ế', 'ề', 'ể', 'ễ', 'ệ'][t],
            ('i', VowelMod::None) => ['i', 'í', 'ì', 'ỉ', 'ĩ', 'ị'][t],
            ('o', VowelMod::None) => ['o', 'ó', 'ò', 'ỏ', 'õ', 'ọ'][t],
            ('o', VowelMod::Circumflex) => ['ô', 'ố', 'ồ', 'ổ', 'ỗ', 'ộ'][t],
            ('o', VowelMod::Horn) => ['ơ', 'ớ', 'ờ', 'ở', 'ỡ', 'ợ'][t],
            ('u', VowelMod::None) => ['u', 'ú', 'ù', 'ủ', 'ũ', 'ụ'][t],
            ('u', VowelMod::Horn) => ['ư', 'ứ', 'ừ', 'ử', 'ữ', 'ự'][t],
            ('y', VowelMod::None) => ['y', 'ý', 'ỳ', 'ỷ', 'ỹ', 'ỵ'][t],
            _ => self.base,
        };

        if self.uppercase {
            ch.to_uppercase().next().unwrap_or(ch)
        } else {
            ch
        }
    }

    pub fn modded_vowel_char(&self) -> char {
        match (self.base, self.vmod) {
            ('a', VowelMod::Breve) => 'ă',
            ('a', VowelMod::Circumflex) => 'â',
            ('e', VowelMod::Circumflex) => 'ê',
            ('o', VowelMod::Circumflex) => 'ô',
            ('o', VowelMod::Horn) => 'ơ',
            ('u', VowelMod::Horn) => 'ư',
            _ => self.base,
        }
    }
}
