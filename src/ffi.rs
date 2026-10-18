// SPDX-License-Identifier: GPL-3.0-or-later

use crate::config::{ImeConfig, SmoothMode};
use crate::engine::ImeEngine;
use libc::c_char;
use std::ffi::CString;
use std::fs;
use std::ptr;
use std::time::{Instant, SystemTime};

#[repr(C)]
pub struct FfiKeyResult {
    pub handled: bool,
    pub commit_text: *mut c_char,
    pub preedit_text: *mut c_char,
    pub underline_preedit: bool,
    pub direct_mode: bool,
    pub commit_delay_ms: u32,
}

pub struct VietImeContext {
    pub engine: ImeEngine,
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
        if self.last_config_check.elapsed().as_secs() >= 1 {
            self.last_config_check = Instant::now();
            let mtime = current_config_mtime();
            if mtime != self.last_config_mtime {
                self.last_config_mtime = mtime;
                self.engine.config = ImeConfig::load_from_disk();
            }
        }
    }

    fn build_result(
        &self,
        handled: bool,
        commit: Option<String>,
        preedit: Option<String>,
    ) -> FfiKeyResult {
        let cfg = &self.engine.config;
        let direct_mode = cfg.smooth_mode == SmoothMode::Smooth;
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
            underline_preedit: true,
            direct_mode: false,
            commit_delay_ms: 0,
        };
    };

    let committed = c.engine.finalize_word();
    c.build_result(committed.is_some(), committed, None)
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
        underline_preedit: true,
        direct_mode: false,
        commit_delay_ms: 0,
    };

    if is_release || ctx.is_null() {
        return empty_result;
    }

    let c = &mut *ctx;
    c.maybe_hot_reload_config();

    let non_shift_mods = state & (FCITX_CONTROL_MASK | FCITX_ALT_MASK | FCITX_SUPER_MASK);
    if non_shift_mods != 0 {
        if (state & FCITX_CONTROL_MASK) != 0
            && (state & FCITX_SHIFT_MASK) != 0
            && (keysym == b'S' as u32 || keysym == b's' as u32)
        {
            let committed = c.engine.finalize_word();
            c.engine.config.enabled = !c.engine.config.enabled;
            let _ = c.engine.config.save_to_disk();
            return c.build_result(true, committed, None);
        }

        if !c.engine.is_empty() {
            let committed = c.engine.finalize_word();
            return c.build_result(false, committed, None);
        }
        return empty_result;
    }

    if keysym == KEYSYM_BACKSPACE {
        let res = c.engine.process_backspace();
        return c.build_result(res.handled, res.commit, Some(res.preedit));
    }

    if matches!(
        keysym,
        KEYSYM_RETURN | KEYSYM_KP_ENTER | KEYSYM_TAB | KEYSYM_ESCAPE
    ) || (0xff00..=0xffff).contains(&keysym)
    {
        if !c.engine.is_empty() {
            let committed = c.engine.finalize_word();
            return c.build_result(false, committed, None);
        }
        return empty_result;
    }

    if (0x20..=0x7e).contains(&keysym) {
        let ch = keysym as u8 as char;
        let res = c.engine.process_char(ch);
        return c.build_result(res.handled, res.commit, Some(res.preedit));
    }

    if !c.engine.is_empty() {
        let committed = c.engine.finalize_word();
        return c.build_result(false, committed, None);
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
