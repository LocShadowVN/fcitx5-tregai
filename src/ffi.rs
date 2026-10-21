// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::{ImeConfig, SmoothMode};
use crate::engine::ImeEngine;
use libc::c_char;
use std::ffi::{CStr, CString};
use std::fs;
use std::ptr;
use std::time::{Duration, Instant, SystemTime};

#[repr(C)]
pub struct FfiKeyResult {
    pub handled: bool,
    pub commit_text: *mut c_char,
    pub preedit_text: *mut c_char,
    pub backspace_count: u32,
    pub underline_preedit: bool,
    pub direct_mode: bool,
    pub commit_delay_ms: u32,
}

pub struct VietImeContext {
    pub engine: ImeEngine,
    pub displayed_word: String,
    pub current_program: String,
    last_config_check: Instant,
    last_config_mtime: Option<SystemTime>,
}

const FCITX_SHIFT_MASK: u32 = 1 << 0;
const FCITX_CONTROL_MASK: u32 = 1 << 2;
const FCITX_ALT_MASK: u32 = 1 << 3;
const FCITX_SUPER_MASK: u32 = 1 << 6;

const KEYSYM_BACKSPACE: u32 = 0xff08;
const KEYSYM_TAB: u32 = 0xff09;
const KEYSYM_RETURN: u32 = 0xff0d;
const KEYSYM_ESCAPE: u32 = 0xff1b;
const KEYSYM_KP_ENTER: u32 = 0xff8d;

fn into_c_string_ptr(s: Option<String>) -> *mut c_char {
    match s {
        Some(text) if !text.is_empty() => CString::new(text)
            .map(CString::into_raw)
            .unwrap_or(ptr::null_mut()),
        _ => ptr::null_mut(),
    }
}

fn current_config_mtime() -> Option<SystemTime> {
    let path = ImeConfig::config_path()?;
    fs::metadata(path).ok()?.modified().ok()
}

impl VietImeContext {
    fn maybe_hot_reload_config(&mut self) {
        if self.last_config_check.elapsed() >= Duration::from_millis(200) {
            self.last_config_check = Instant::now();
            let mtime = current_config_mtime();
            if mtime != self.last_config_mtime {
                self.last_config_mtime = mtime;
                self.engine.config = ImeConfig::load_from_disk();
                self.displayed_word.clear();
            }
        }
    }

    fn is_direct_mode(&self) -> bool {
        matches!(
            self.engine.config.smooth_mode,
            SmoothMode::Smooth | SmoothMode::SlowCompat
        )
    }

    fn transition_direct_display(&mut self, target_display: &str) -> (u32, Option<String>) {
        let old_chars: Vec<char> = self.displayed_word.chars().collect();
        let new_chars: Vec<char> = target_display.chars().collect();

        let common = old_chars
            .iter()
            .zip(new_chars.iter())
            .take_while(|(a, b)| a == b)
            .count();

        let backspace_count = (old_chars.len() - common) as u32;
        let suffix: String = new_chars[common..].iter().collect();
        self.displayed_word = target_display.to_string();

        let commit = if suffix.is_empty() {
            None
        } else {
            Some(suffix)
        };
        (backspace_count, commit)
    }

    fn build_result(
        &self,
        handled: bool,
        commit: Option<String>,
        preedit: Option<String>,
        backspace_count: u32,
    ) -> FfiKeyResult {
        let cfg = &self.engine.config;
        let direct_mode = self.is_direct_mode();
        let underline_preedit = cfg.underline_preedit && !direct_mode;
        let commit_delay_ms = if cfg.smooth_mode == SmoothMode::SlowCompat {
            cfg.commit_delay_ms
        } else {
            0
        };

        FfiKeyResult {
            handled,
            commit_text: into_c_string_ptr(commit),
            preedit_text: into_c_string_ptr(preedit),
            backspace_count,
            underline_preedit,
            direct_mode,
            commit_delay_ms,
        }
    }
}

#[no_mangle]
pub extern "C" fn viet_ime_context_new() -> *mut VietImeContext {
    let config = ImeConfig::load_from_disk();
    let ctx = Box::new(VietImeContext {
        engine: ImeEngine::new(config),
        displayed_word: String::with_capacity(32),
        current_program: String::new(),
        last_config_check: Instant::now(),
        last_config_mtime: current_config_mtime(),
    });
    Box::into_raw(ctx)
}

/// Frees a `VietImeContext` previously allocated by `viet_ime_context_new`.
///
/// # Safety
/// `ctx` must be either null or a valid pointer returned by `viet_ime_context_new`
/// that has not yet been freed.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_context_free(ctx: *mut VietImeContext) {
    if !ctx.is_null() {
        drop(Box::from_raw(ctx));
    }
}

/// Resets the internal syllable state of `VietImeContext`.
///
/// # Safety
/// `ctx` must be either null or a valid pointer to `VietImeContext`.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_context_reset(ctx: *mut VietImeContext) {
    if let Some(c) = ctx.as_mut() {
        c.engine.clear();
        c.displayed_word.clear();
    }
}

/// Sets the active client application program name (e.g. from `ic->program()`).
///
/// # Safety
/// `ctx` must be a valid pointer to `VietImeContext`, and `program` may be null or a valid C string.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_context_set_program(
    ctx: *mut VietImeContext,
    program: *const c_char,
) {
    let Some(c) = ctx.as_mut() else {
        return;
    };
    if program.is_null() {
        c.current_program.clear();
    } else if let Ok(s) = CStr::from_ptr(program).to_str() {
        c.current_program = s.to_string();
    }
}

/// Reloads configuration from disk for the given `VietImeContext`.
///
/// # Safety
/// `ctx` must be either null or a valid pointer to `VietImeContext`.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_context_reload_config(ctx: *mut VietImeContext) {
    if let Some(c) = ctx.as_mut() {
        c.engine.config = ImeConfig::load_from_disk();
        c.last_config_mtime = current_config_mtime();
        c.engine.clear();
        c.displayed_word.clear();
    }
}

/// Flushes any pending syllable in the buffer and returns an `FfiKeyResult`.
///
/// # Safety
/// `ctx` must be either null or a valid pointer to `VietImeContext`.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_context_flush(ctx: *mut VietImeContext) -> FfiKeyResult {
    let Some(c) = ctx.as_mut() else {
        return FfiKeyResult {
            handled: false,
            commit_text: ptr::null_mut(),
            preedit_text: ptr::null_mut(),
            backspace_count: 0,
            underline_preedit: true,
            direct_mode: false,
            commit_delay_ms: 0,
        };
    };

    if c.is_direct_mode() {
        let finalized = c.engine.finalize_word().unwrap_or_default();
        let (bs, diff_commit) = c.transition_direct_display(&finalized);
        c.displayed_word.clear();
        return c.build_result(bs > 0 || diff_commit.is_some(), diff_commit, None, bs);
    }

    let committed = c.engine.finalize_word();
    c.displayed_word.clear();
    c.build_result(committed.is_some(), committed, None, 0)
}

/// Processes a single key event from Fcitx5.
///
/// # Safety
/// `ctx` must be either null or a valid pointer to `VietImeContext`.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_context_process_key(
    ctx: *mut VietImeContext,
    keysym: u32,
    state: u32,
    is_release: bool,
) -> FfiKeyResult {
    let empty_result = FfiKeyResult {
        handled: false,
        commit_text: ptr::null_mut(),
        preedit_text: ptr::null_mut(),
        backspace_count: 0,
        underline_preedit: true,
        direct_mode: false,
        commit_delay_ms: 0,
    };

    if is_release || ctx.is_null() {
        return empty_result;
    }

    let c = &mut *ctx;
    c.maybe_hot_reload_config();

    if c.engine.config.is_app_excluded(&c.current_program) {
        c.engine.clear();
        c.displayed_word.clear();
        return empty_result;
    }

    let direct = c.is_direct_mode();

    let non_shift_mods = state & (FCITX_CONTROL_MASK | FCITX_ALT_MASK | FCITX_SUPER_MASK);
    if non_shift_mods != 0 {
        if (state & FCITX_CONTROL_MASK) != 0
            && (state & FCITX_SHIFT_MASK) != 0
            && (keysym == b'S' as u32 || keysym == b's' as u32)
        {
            let flushed = viet_ime_context_flush(ctx);
            c.engine.config.enabled = !c.engine.config.enabled;
            let _ = c.engine.config.save_to_disk();
            return FfiKeyResult {
                handled: true,
                ..flushed
            };
        }

        if !c.engine.is_empty() {
            let mut flushed = viet_ime_context_flush(ctx);
            flushed.handled = false;
            return flushed;
        }
        return empty_result;
    }

    if keysym == KEYSYM_BACKSPACE {
        let res = c.engine.process_backspace();
        if !res.handled {
            c.displayed_word.clear();
            return empty_result;
        }
        if direct {
            let (bs, diff_commit) = c.transition_direct_display(&res.preedit);
            if bs == 1 && diff_commit.is_none() {
                return c.build_result(false, None, None, 0);
            }
            return c.build_result(true, diff_commit, None, bs);
        }
        return c.build_result(true, res.commit, Some(res.preedit), 0);
    }

    if matches!(
        keysym,
        KEYSYM_RETURN | KEYSYM_KP_ENTER | KEYSYM_TAB | KEYSYM_ESCAPE
    ) || (0xff00..=0xffff).contains(&keysym)
    {
        if !c.engine.is_empty() {
            let mut flushed = viet_ime_context_flush(ctx);
            flushed.handled = false;
            return flushed;
        }
        c.displayed_word.clear();
        return empty_result;
    }

    if (0x20..=0x7e).contains(&keysym) {
        let ch = keysym as u8 as char;
        let res = c.engine.process_char(ch);
        if !res.handled {
            c.displayed_word.clear();
            return empty_result;
        }

        if direct {
            if let Some(committed) = res.commit {
                let (bs, diff_commit) = c.transition_direct_display(&committed);
                c.displayed_word.clear();
                return c.build_result(true, diff_commit, None, bs);
            }
            let (bs, diff_commit) = c.transition_direct_display(&res.preedit);
            return c.build_result(true, diff_commit, None, bs);
        }

        c.displayed_word.clear();
        return c.build_result(true, res.commit, Some(res.preedit), 0);
    }

    if !c.engine.is_empty() {
        let mut flushed = viet_ime_context_flush(ctx);
        flushed.handled = false;
        return flushed;
    }

    empty_result
}

/// Frees heap-allocated strings inside `FfiKeyResult`.
///
/// # Safety
/// `res` must have been returned by `viet_ime_context_process_key` or `viet_ime_context_flush`
/// and must not have been freed already.
#[no_mangle]
pub unsafe extern "C" fn viet_ime_free_result(res: FfiKeyResult) {
    if !res.commit_text.is_null() {
        drop(CString::from_raw(res.commit_text));
    }
    if !res.preedit_text.is_null() {
        drop(CString::from_raw(res.preedit_text));
    }
}

#[cfg(feature = "fcitx5-plugin")]
extern "C" {
    fn rust_fcitx5_addon_factory_instance() -> *mut libc::c_void;
}

/// Entrypoint symbol resolved by Fcitx5 `AddonLoader` via `dlsym`.
///
/// # Safety
/// Called directly by Fcitx5's dynamic loader on the main thread.
#[cfg(feature = "fcitx5-plugin")]
#[no_mangle]
pub unsafe extern "C" fn fcitx_addon_factory_instance() -> *mut libc::c_void {
    rust_fcitx5_addon_factory_instance()
}
