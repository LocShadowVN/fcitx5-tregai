// SPDX-License-Identifier: GPL-3.0-or-later

pub mod config;
pub mod engine;
pub mod ffi;
pub mod syllable;
pub mod unicode;

#[cfg(test)]
mod tests {
    use crate::config::{ImeConfig, InputMethod, SmoothMode};
    use crate::engine::ImeEngine;
    use crate::ffi::{
        viet_ime_context_free, viet_ime_context_new, viet_ime_context_process_key,
        viet_ime_free_result,
    };
    use std::ffi::CStr;

    pub fn simulate_typing(engine: &mut ImeEngine, input: &str) -> String {
        let mut output = String::new();
        for ch in input.chars() {
            let res = engine.process_char(ch);
            if let Some(committed) = res.commit {
                output.push_str(&committed);
            } else if !res.handled {
                output.push(ch);
            }
        }
        if let Some(rem) = engine.finalize_word() {
            output.push_str(&rem);
        }
        output
    }

    #[test]
    fn test_tregai_sentences_and_tones() {
        let mut engine = ImeEngine::new(ImeConfig::default());
        let text = simulate_typing(
            &mut engine,
            "Booj gox Tre Gai sieeu muwowtj treen Linux!",
        );
        assert_eq!(text, "Bộ gõ Tre Gai siêu mượt trên Linux!");
    }

    #[test]
    fn test_all_four_ways_to_type_uong_horn() {
        let mut engine = ImeEngine::new(ImeConfig::default());
        assert_eq!(simulate_typing(&mut engine, "tuongw"), "tương");
        assert_eq!(simulate_typing(&mut engine, "tuowng"), "tương");
        assert_eq!(simulate_typing(&mut engine, "tuwong"), "tương");
        assert_eq!(simulate_typing(&mut engine, "tuwowng"), "tương");

        assert_eq!(simulate_typing(&mut engine, "huow"), "huơ");
        assert_eq!(simulate_typing(&mut engine, "thuowr"), "thuở");
        assert_eq!(simulate_typing(&mut engine, "huowng"), "hương");
        assert_eq!(simulate_typing(&mut engine, "thuowng"), "thương");
        assert_eq!(simulate_typing(&mut engine, "huowu"), "hươu");
        assert_eq!(simulate_typing(&mut engine, "ruwowuj"), "rượu");
    }

    #[test]
    fn test_qu_and_gi_disambiguation() {
        let mut engine = ImeEngine::new(ImeConfig::default());
        assert_eq!(simulate_typing(&mut engine, "quas"), "quá");
        assert_eq!(simulate_typing(&mut engine, "quys"), "quý");
        assert_eq!(simulate_typing(&mut engine, "quyeenf"), "quyền");
        assert_eq!(simulate_typing(&mut engine, "quawjps"), "quắp");
        assert_eq!(simulate_typing(&mut engine, "muaw"), "mưa");
        assert_eq!(simulate_typing(&mut engine, "hoacwj"), "hoặc");

        assert_eq!(simulate_typing(&mut engine, "gif"), "gì");
        assert_eq!(simulate_typing(&mut engine, "ginf"), "gìn");
        assert_eq!(simulate_typing(&mut engine, "giaf"), "già");
        assert_eq!(simulate_typing(&mut engine, "giaos"), "giáo");
        assert_eq!(simulate_typing(&mut engine, "gieengs"), "giếng");
        assert_eq!(simulate_typing(&mut engine, "giuongwf"), "giường");
        assert_eq!(simulate_typing(&mut engine, "giuwx"), "giữ");
    }

    #[test]
    fn test_modern_vs_traditional_tone_placement() {
        let old_cfg = ImeConfig {
            modern_tone: false,
            ..ImeConfig::default()
        };
        let mut old_engine = ImeEngine::new(old_cfg);
        assert_eq!(simulate_typing(&mut old_engine, "hoaf"), "hòa");
        assert_eq!(simulate_typing(&mut old_engine, "khoer"), "khỏe");
        assert_eq!(simulate_typing(&mut old_engine, "thuyr"), "thủy");
        assert_eq!(simulate_typing(&mut old_engine, "hoafn"), "hoàn");
        assert_eq!(simulate_typing(&mut old_engine, "tuyts"), "tuýt");

        let mod_cfg = ImeConfig {
            modern_tone: true,
            ..ImeConfig::default()
        };
        let mut mod_engine = ImeEngine::new(mod_cfg);
        assert_eq!(simulate_typing(&mut mod_engine, "hoaf"), "hoà");
        assert_eq!(simulate_typing(&mut mod_engine, "khoer"), "khoẻ");
        assert_eq!(simulate_typing(&mut mod_engine, "thuyr"), "thuỷ");
        assert_eq!(simulate_typing(&mut mod_engine, "hoafn"), "hoàn");
    }

    #[test]
    fn test_quick_consonants_and_auto_capitalize() {
        let cfg = ImeConfig {
            quick_start_consonant: true,
            quick_end_consonant: true,
            auto_capitalize: true,
            double_space_period: true,
            ..ImeConfig::default()
        };

        let mut engine = ImeEngine::new(cfg);
        assert_eq!(simulate_typing(&mut engine, "fog "), "phong ");
        assert_eq!(simulate_typing(&mut engine, "jaf "), "già ");
        assert_eq!(simulate_typing(&mut engine, "was "), "quá ");
        assert_eq!(simulate_typing(&mut engine, "nhah "), "nhanh ");
        assert_eq!(simulate_typing(&mut engine, "saks "), "sách ");

        assert_eq!(
            simulate_typing(&mut engine, "xin chaof. vieetj nam"),
            "xin chào. Việt nam"
        );
    }

    #[test]
    fn test_gaming_mode_wasd_protection() {
        let cfg = ImeConfig {
            smooth_mode: SmoothMode::Gaming,
            ..ImeConfig::default()
        };
        let mut engine = ImeEngine::new(cfg);

        assert_eq!(simulate_typing(&mut engine, "w a s d"), "w a s d");
        assert_eq!(simulate_typing(&mut engine, "muaw"), "mưa");
    }

    #[test]
    fn test_vni_and_viqr_modes() {
        let vni_cfg = ImeConfig {
            method: InputMethod::Vni,
            ..ImeConfig::default()
        };
        let mut vni = ImeEngine::new(vni_cfg);
        assert_eq!(
            simulate_typing(&mut vni, "Tre Gai Vie65t Nam"),
            "Tre Gai Việt Nam"
        );

        let viqr_cfg = ImeConfig {
            method: InputMethod::Viqr,
            ..ImeConfig::default()
        };
        let mut viqr = ImeEngine::new(viqr_cfg);
        assert_eq!(
            simulate_typing(&mut viqr, "Tre Gai Vie^.t Nam"),
            "Tre Gai Việt Nam"
        );
    }

    #[test]
    fn test_english_auto_restore() {
        let mut engine = ImeEngine::new(ImeConfig::default());
        let words = "windows linux text next class staff free struct string user house focus";
        assert_eq!(simulate_typing(&mut engine, words), words);
    }

    #[test]
    fn test_ffi_bridge_execution() {
        unsafe {
            let ctx = viet_ime_context_new();
            assert!(!ctx.is_null());

            for &ch in b"tregai" {
                let res = viet_ime_context_process_key(ctx, ch as u32, 0, false);
                assert!(res.handled);
                viet_ime_free_result(res);
            }

            let space = viet_ime_context_process_key(ctx, b' ' as u32, 0, false);
            assert!(space.handled);
            let s = CStr::from_ptr(space.commit_text).to_str().unwrap();
            assert_eq!(s, "tregai ");
            viet_ime_free_result(space);

            viet_ime_context_free(ctx);
        }
    }
}
