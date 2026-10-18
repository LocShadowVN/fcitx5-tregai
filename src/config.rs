// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMethod {
    Telex,
    SimpleTelex,
    Vni,
    Viqr,
}

impl InputMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telex => "telex",
            Self::SimpleTelex => "simple_telex",
            Self::Vni => "vni",
            Self::Viqr => "viqr",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "vni" => Self::Vni,
            "viqr" => Self::Viqr,
            "simple_telex" | "simpletelex" => Self::SimpleTelex,
            _ => Self::Telex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmoothMode {
    UltraSmooth,
    Smooth,
    SlowCompat,
    Gaming,
}

impl SmoothMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UltraSmooth => "ultra_smooth",
            Self::Smooth => "smooth",
            Self::SlowCompat => "slow_compat",
            Self::Gaming => "gaming",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "smooth" | "direct" => Self::Smooth,
            "slow_compat" | "slow" | "compat" => Self::SlowCompat,
            "gaming" | "game" => Self::Gaming,
            _ => Self::UltraSmooth,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImeConfig {
    pub enabled: bool,
    pub method: InputMethod,
    pub smooth_mode: SmoothMode,
    pub commit_delay_ms: u32,
    pub underline_preedit: bool,
    pub modern_tone: bool,
    pub spell_check: bool,
    pub free_tone: bool,
    pub bracket_shortcuts: bool,
    pub quick_start_consonant: bool,
    pub quick_end_consonant: bool,
    pub auto_capitalize: bool,
    pub double_space_period: bool,
    pub allow_foreign_consonants: bool,
    pub enable_macros: bool,
    pub macros: BTreeMap<String, String>,
}

impl Default for ImeConfig {
    fn default() -> Self {
        let mut macros = BTreeMap::new();
        macros.insert("vn".to_string(), "Việt Nam".to_string());
        macros.insert("ko".to_string(), "không".to_string());
        macros.insert("dc".to_string(), "được".to_string());
        macros.insert("hn".to_string(), "Hà Nội".to_string());
        macros.insert("tphcm".to_string(), "Thành phố Hồ Chí Minh".to_string());
        macros.insert("tg".to_string(), "Tre Gai".to_string());

        Self {
            enabled: true,
            method: InputMethod::Telex,
            smooth_mode: SmoothMode::UltraSmooth,
            commit_delay_ms: 15,
            underline_preedit: true,
            modern_tone: false,
            spell_check: true,
            free_tone: true,
            bracket_shortcuts: true,
            quick_start_consonant: false,
            quick_end_consonant: false,
            auto_capitalize: false,
            double_space_period: false,
            allow_foreign_consonants: false,
            enable_macros: true,
            macros,
        }
    }
}

impl ImeConfig {
    pub fn config_path() -> Option<PathBuf> {
        if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return Some(PathBuf::from(xdg).join("fcitx5-tregai/config.ini"));
            }
        }
        if let Ok(home) = env::var("HOME") {
            if !home.is_empty() {
                return Some(PathBuf::from(home).join(".config/fcitx5-tregai/config.ini"));
            }
        }
        None
    }

    pub fn load_from_disk() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        let Ok(content) = fs::read_to_string(path) else {
            let default_cfg = Self::default();
            let _ = default_cfg.save_to_disk();
            return default_cfg;
        };
        Self::parse_ini(&content)
    }

    pub fn save_to_disk(&self) -> io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "HOME or XDG_CONFIG_HOME environment variable is not set",
            ));
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_ini())
    }

    pub fn to_ini(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push_str("# Configuration file for fcitx5-tregai\n\n");
        out.push_str("[general]\n");
        out.push_str(&format!("enabled = {}\n", self.enabled));
        out.push_str(&format!("method = {}\n", self.method.as_str()));
        out.push_str(&format!("smooth_mode = {}\n", self.smooth_mode.as_str()));
        out.push_str(&format!("commit_delay_ms = {}\n", self.commit_delay_ms));
        out.push_str(&format!("underline_preedit = {}\n", self.underline_preedit));
        out.push_str(&format!("modern_tone = {}\n", self.modern_tone));
        out.push_str(&format!("spell_check = {}\n", self.spell_check));
        out.push_str(&format!("free_tone = {}\n", self.free_tone));
        out.push_str(&format!("bracket_shortcuts = {}\n", self.bracket_shortcuts));
        out.push_str(&format!(
            "quick_start_consonant = {}\n",
            self.quick_start_consonant
        ));
        out.push_str(&format!(
            "quick_end_consonant = {}\n",
            self.quick_end_consonant
        ));
        out.push_str(&format!("auto_capitalize = {}\n", self.auto_capitalize));
        out.push_str(&format!(
            "double_space_period = {}\n",
            self.double_space_period
        ));
        out.push_str(&format!(
            "allow_foreign_consonants = {}\n",
            self.allow_foreign_consonants
        ));
        out.push_str(&format!("enable_macros = {}\n\n", self.enable_macros));

        out.push_str("[macros]\n");
        for (k, v) in &self.macros {
            out.push_str(&format!("{} = {}\n", k, v));
        }
        out
    }

    pub fn parse_ini(content: &str) -> Self {
        let mut cfg = Self::default();
        let mut in_macros = false;
        let mut macros_cleared = false;

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                let section = line[1..line.len() - 1].trim().to_ascii_lowercase();
                in_macros = section == "macros";
                if in_macros && !macros_cleared {
                    cfg.macros.clear();
                    macros_cleared = true;
                }
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = k.trim();
            let val = v.trim();

            if in_macros {
                if !key.is_empty() && !val.is_empty() {
                    cfg.macros.insert(key.to_lowercase(), val.to_string());
                }
            } else {
                let b = matches!(val.to_ascii_lowercase().as_str(), "true" | "1" | "yes");
                match key.to_ascii_lowercase().as_str() {
                    "enabled" => cfg.enabled = b,
                    "method" => cfg.method = InputMethod::parse(val),
                    "smooth_mode" => cfg.smooth_mode = SmoothMode::parse(val),
                    "commit_delay_ms" => {
                        cfg.commit_delay_ms = val.parse::<u32>().unwrap_or(15).min(100)
                    }
                    "underline_preedit" => cfg.underline_preedit = b,
                    "modern_tone" => cfg.modern_tone = b,
                    "spell_check" => cfg.spell_check = b,
                    "free_tone" => cfg.free_tone = b,
                    "bracket_shortcuts" => cfg.bracket_shortcuts = b,
                    "quick_start_consonant" => cfg.quick_start_consonant = b,
                    "quick_end_consonant" => cfg.quick_end_consonant = b,
                    "auto_capitalize" => cfg.auto_capitalize = b,
                    "double_space_period" => cfg.double_space_period = b,
                    "allow_foreign_consonants" => cfg.allow_foreign_consonants = b,
                    "enable_macros" => cfg.enable_macros = b,
                    _ => {}
                }
            }
        }
        cfg
    }

    pub fn expand_macro(&self, word: &str) -> Option<String> {
        if !self.enable_macros || word.is_empty() {
            return None;
        }
        let lower = word.to_lowercase();
        let target = self.macros.get(&lower)?;

        let chars: Vec<char> = word.chars().collect();
        let all_upper = chars.len() > 1 && chars.iter().all(|c| c.is_uppercase());
        let first_upper = chars.first().is_some_and(|c| c.is_uppercase());

        if all_upper {
            Some(target.to_uppercase())
        } else if first_upper {
            let mut result = String::with_capacity(target.len());
            let mut target_chars = target.chars();
            if let Some(first) = target_chars.next() {
                for u in first.to_uppercase() {
                    result.push(u);
                }
                for c in target_chars {
                    result.push(c);
                }
            }
            Some(result)
        } else {
            Some(target.clone())
        }
    }
}
