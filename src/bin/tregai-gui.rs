// SPDX-License-Identifier: GPL-3.0-or-later

use fcitx5_tregai::config::{ImeConfig, InputMethod, SmoothMode};
use fcitx5_tregai::engine::ImeEngine;
use fcitx5_tregai::unicode::Charset;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const ADDON_CONF_CONTENT: &str = include_str!("../../data/tregai-addon.conf");
const IM_CONF_CONTENT: &str = include_str!("../../data/tregai.conf");
const DESKTOP_ENTRY_CONTENT: &str = include_str!("../../data/tregai-gui.desktop");

struct AppState {
    config: ImeConfig,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("tregai-gui {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let state = Arc::new(Mutex::new(AppState {
        config: ImeConfig::load_from_disk(),
    }));

    let listener = TcpListener::bind("127.0.0.1:0")
        .or_else(|_| TcpListener::bind("127.0.0.1:18492"))
        .expect("Failed to bind local configuration server port");

    let addr = listener
        .local_addr()
        .expect("Failed to retrieve local server address");
    let url = format!("http://127.0.0.1:{}", addr.port());

    println!("Tre Gai Configuration Server running at: {url}");

    #[cfg(feature = "native-webview")]
    {
        let server_state = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let st = Arc::clone(&server_state);
                thread::spawn(move || handle_client(stream, &st));
            }
        });
        run_native_webview(&url);
    }

    #[cfg(not(feature = "native-webview"))]
    {
        open_browser_window(&url);
        for stream in listener.incoming().flatten() {
            let st = Arc::clone(&state);
            thread::spawn(move || handle_client(stream, &st));
        }
    }
}

#[cfg(feature = "native-webview")]
fn run_native_webview(url: &str) {
    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoop};
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Bộ gõ Tre Gai (fcitx5-tregai)")
        .with_inner_size(tao::dpi::LogicalSize::new(980.0, 720.0))
        .build(&event_loop)
        .expect("Failed to create native window");

    let _webview = WebViewBuilder::new()
        .with_url(url)
        .build(&window)
        .expect("Failed to initialize WebKit2GTK WebView");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            *control_flow = ControlFlow::Exit;
        }
    });
}

#[cfg(not(feature = "native-webview"))]
fn open_browser_window(url: &str) {
    if env::var("DISPLAY").is_err() && env::var("WAYLAND_DISPLAY").is_err() {
        return;
    }

    let app_arg = format!("--app={url}");
    let browsers = [
        "google-chrome",
        "chromium",
        "chromium-browser",
        "brave-browser",
        "microsoft-edge",
    ];

    for bin in browsers {
        if Command::new(bin)
            .arg(&app_arg)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return;
        }
    }

    let _ = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn parse_content_length(headers: &str) -> usize {
    for line in headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                if let Ok(len) = v.trim().parse::<usize>() {
                    return len;
                }
            }
        }
    }
    0
}

fn handle_client(mut stream: TcpStream, state: &Arc<Mutex<AppState>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;

    while let Ok(n) = stream.read(&mut chunk) {
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if header_end.is_none() {
            if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
                let end = pos + 4;
                header_end = Some(end);
                let header_str = String::from_utf8_lossy(&buf[..pos]);
                content_length = parse_content_length(&header_str);
            }
        }
        if let Some(end) = header_end {
            if buf.len() >= end + content_length {
                break;
            }
        }
        if buf.len() > 256 * 1024 {
            break;
        }
    }

    let Some(end) = header_end else {
        return;
    };

    let header_str = String::from_utf8_lossy(&buf[..end - 4]);
    let first_line = header_str.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");
    let body = String::from_utf8_lossy(&buf[end..]).into_owned();

    match (method, path) {
        ("GET", "/") => send_response(&mut stream, 200, "text/html; charset=utf-8", INDEX_HTML),
        ("GET", "/api/state") => {
            let json = build_state_json(state);
            send_response(&mut stream, 200, "application/json; charset=utf-8", &json);
        }
        ("POST", "/api/save") => {
            let resp = handle_save_config(state, &body);
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        ("POST", "/api/simulate") => {
            let resp = handle_simulate(state, &body);
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        ("POST", "/api/install-user") => {
            let (ok, msg) = install_user_local();
            let resp = format!(
                "{{\"ok\":{},\"message\":\"{}\"}}",
                ok,
                escape_json_str(&msg)
            );
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        ("POST", "/api/install-system") => {
            let (ok, msg) = install_system_wide();
            let resp = format!(
                "{{\"ok\":{},\"message\":\"{}\"}}",
                ok,
                escape_json_str(&msg)
            );
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        ("POST", "/api/uninstall") => {
            let (ok, msg) = uninstall_all();
            let resp = format!(
                "{{\"ok\":{},\"message\":\"{}\"}}",
                ok,
                escape_json_str(&msg)
            );
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        ("POST", "/api/restart-fcitx5") => {
            let ok = restart_fcitx5();
            let msg = if ok {
                "Đã khởi động lại Fcitx5 thành công."
            } else {
                "Không thể khởi động lại Fcitx5 (hãy kiểm tra gói fcitx5)."
            };
            let resp = format!("{{\"ok\":{ok},\"message\":\"{}\"}}", escape_json_str(msg));
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        ("POST", "/api/open-configtool") => {
            let ok = Command::new("fcitx5-configtool")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok();
            let msg = if ok {
                "Đã mở fcitx5-configtool."
            } else {
                "Không tìm thấy fcitx5-configtool trên hệ thống."
            };
            let resp = format!("{{\"ok\":{ok},\"message\":\"{}\"}}", escape_json_str(msg));
            send_response(&mut stream, 200, "application/json; charset=utf-8", &resp);
        }
        _ => send_response(&mut stream, 404, "text/plain; charset=utf-8", "Not Found"),
    }
}

fn send_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let status_text = match status {
        200 => "OK",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {status_text}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &input[i + 1..i + 3];
            if let Ok(val) = u8::from_str_radix(hex, 16) {
                out.push(val);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_form_urlencoded(body: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(url_decode(k), url_decode(v));
        }
    }
    map
}

fn build_state_json(state: &Arc<Mutex<AppState>>) -> String {
    let guard = state.lock().expect("Mutex poisoned");
    let cfg = &guard.config;

    let mut macros_text = String::new();
    for (k, v) in &cfg.macros {
        macros_text.push_str(&format!("{k} = {v}\n"));
    }

    let excluded_text = cfg.excluded_apps.join(", ");
    let user_installed = is_user_installed();
    let system_installed = is_system_installed();

    format!(
        concat!(
            "{{",
            "\"enabled\":{},",
            "\"method\":\"{}\",",
            "\"charset\":\"{}\",",
            "\"smooth_mode\":\"{}\",",
            "\"commit_delay_ms\":{},",
            "\"underline_preedit\":{},",
            "\"modern_tone\":{},",
            "\"spell_check\":{},",
            "\"auto_restore_english\":{},",
            "\"free_tone\":{},",
            "\"w_to_u\":{},",
            "\"bracket_shortcuts\":{},",
            "\"quick_start_consonant\":{},",
            "\"quick_end_consonant\":{},",
            "\"auto_capitalize\":{},",
            "\"double_space_period\":{},",
            "\"allow_foreign_consonants\":{},",
            "\"enable_macros\":{},",
            "\"macro_in_english_mode\":{},",
            "\"excluded_apps\":\"{}\",",
            "\"macros_text\":\"{}\",",
            "\"user_installed\":{},",
            "\"system_installed\":{}",
            "}}"
        ),
        cfg.enabled,
        cfg.method.as_str(),
        cfg.charset.as_str(),
        cfg.smooth_mode.as_str(),
        cfg.commit_delay_ms,
        cfg.underline_preedit,
        cfg.modern_tone,
        cfg.spell_check,
        cfg.auto_restore_english,
        cfg.free_tone,
        cfg.w_to_u,
        cfg.bracket_shortcuts,
        cfg.quick_start_consonant,
        cfg.quick_end_consonant,
        cfg.auto_capitalize,
        cfg.double_space_period,
        cfg.allow_foreign_consonants,
        cfg.enable_macros,
        cfg.macro_in_english_mode,
        escape_json_str(&excluded_text),
        escape_json_str(&macros_text),
        user_installed,
        system_installed
    )
}

fn handle_save_config(state: &Arc<Mutex<AppState>>, body: &str) -> String {
    let params = parse_form_urlencoded(body);
    let mut guard = state.lock().expect("Mutex poisoned");
    let cfg = &mut guard.config;

    let parse_bool = |key: &str, default: bool| -> bool {
        params
            .get(key)
            .map(|v| matches!(v.as_str(), "true" | "1" | "on" | "yes"))
            .unwrap_or(default)
    };

    cfg.enabled = parse_bool("enabled", cfg.enabled);
    if let Some(m) = params.get("method") {
        cfg.method = InputMethod::parse(m);
    }
    if let Some(c) = params.get("charset") {
        cfg.charset = Charset::parse(c);
    }
    if let Some(s) = params.get("smooth_mode") {
        cfg.smooth_mode = SmoothMode::parse(s);
    }
    if let Some(d) = params.get("commit_delay_ms") {
        cfg.commit_delay_ms = d.parse::<u32>().unwrap_or(15).min(100);
    }

    cfg.underline_preedit = parse_bool("underline_preedit", cfg.underline_preedit);
    cfg.modern_tone = parse_bool("modern_tone", cfg.modern_tone);
    cfg.spell_check = parse_bool("spell_check", cfg.spell_check);
    cfg.auto_restore_english = parse_bool("auto_restore_english", cfg.auto_restore_english);
    cfg.free_tone = parse_bool("free_tone", cfg.free_tone);
    cfg.w_to_u = parse_bool("w_to_u", cfg.w_to_u);
    cfg.bracket_shortcuts = parse_bool("bracket_shortcuts", cfg.bracket_shortcuts);
    cfg.quick_start_consonant = parse_bool("quick_start_consonant", cfg.quick_start_consonant);
    cfg.quick_end_consonant = parse_bool("quick_end_consonant", cfg.quick_end_consonant);
    cfg.auto_capitalize = parse_bool("auto_capitalize", cfg.auto_capitalize);
    cfg.double_space_period = parse_bool("double_space_period", cfg.double_space_period);
    cfg.allow_foreign_consonants =
        parse_bool("allow_foreign_consonants", cfg.allow_foreign_consonants);
    cfg.enable_macros = parse_bool("enable_macros", cfg.enable_macros);
    cfg.macro_in_english_mode = parse_bool("macro_in_english_mode", cfg.macro_in_english_mode);

    if let Some(ex) = params.get("excluded_apps") {
        cfg.excluded_apps = ex
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
    }

    if let Some(macros_raw) = params.get("macros_text") {
        cfg.macros.clear();
        cfg.import_unikey_macros(macros_raw);
    }

    match cfg.save_to_disk() {
        Ok(()) => {
            notify_fcitx5_reload();
            "{\"ok\":true,\"message\":\"Đã lưu cấu hình và áp dụng thành công!\"}".to_string()
        }
        Err(e) => format!(
            "{{\"ok\":false,\"message\":\"Lỗi khi lưu cấu hình: {}\"}}",
            escape_json_str(&e.to_string())
        ),
    }
}

fn handle_simulate(state: &Arc<Mutex<AppState>>, body: &str) -> String {
    let params = parse_form_urlencoded(body);
    let raw_input = params.get("text").cloned().unwrap_or_default();

    let cfg = {
        let guard = state.lock().expect("Mutex poisoned");
        guard.config.clone()
    };

    let mut engine = ImeEngine::new(cfg);
    let mut output = String::new();
    for ch in raw_input.chars() {
        let res = engine.process_char(ch);
        let extra_bs = engine.take_pending_backspaces();
        for _ in 0..extra_bs {
            output.pop();
        }
        if let Some(committed) = res.commit {
            output.push_str(&committed);
        } else if !res.handled {
            output.push(ch);
        }
    }
    let preedit = engine.render_current_word();
    output.push_str(&preedit);

    format!(
        "{{\"ok\":true,\"output\":\"{}\",\"preedit\":\"{}\"}}",
        escape_json_str(&output),
        escape_json_str(&preedit)
    )
}

fn detect_system_fcitx5_libdir() -> PathBuf {
    if let Ok(out) = Command::new("pkg-config")
        .args(["--variable=libdir", "Fcitx5Core"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return PathBuf::from(s).join("fcitx5");
            }
        }
    }
    let multiarch = Path::new("/usr/lib/x86_64-linux-gnu/fcitx5");
    if multiarch.exists() {
        return multiarch.to_path_buf();
    }
    PathBuf::from("/usr/lib/fcitx5")
}

fn is_user_installed() -> bool {
    let Ok(home) = env::var("HOME") else {
        return false;
    };
    Path::new(&home)
        .join(".local/lib/fcitx5/libfcitx5_tregai.so")
        .exists()
}

fn is_system_installed() -> bool {
    detect_system_fcitx5_libdir()
        .join("libfcitx5_tregai.so")
        .exists()
}

fn locate_or_build_shared_library() -> Result<PathBuf, String> {
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("libfcitx5_tregai.so");
            if adjacent.exists() {
                return Ok(adjacent);
            }
        }
    }

    let candidates = [
        PathBuf::from("target/release/libfcitx5_tregai.so"),
        PathBuf::from("libfcitx5_tregai.so"),
        detect_system_fcitx5_libdir().join("libfcitx5_tregai.so"),
    ];
    for c in &candidates {
        if c.exists() {
            return Ok(c.clone());
        }
    }

    let status = Command::new("cargo")
        .args(["build", "--release", "--features", "fcitx5-plugin"])
        .status()
        .map_err(|e| format!("Không thể gọi cargo build: {e}"))?;

    if !status.success() {
        return Err(
            "Biên dịch libfcitx5_tregai.so thất bại (kiểm tra gói libfcitx5core-dev).".to_string(),
        );
    }

    let built = PathBuf::from("target/release/libfcitx5_tregai.so");
    if built.exists() {
        Ok(built)
    } else {
        Err("Không tìm thấy tệp libfcitx5_tregai.so sau khi biên dịch.".to_string())
    }
}

fn install_user_local() -> (bool, String) {
    let Ok(home) = env::var("HOME") else {
        return (false, "Biến môi trường HOME chưa được thiết lập.".to_string());
    };
    let so_path = match locate_or_build_shared_library() {
        Ok(p) => p,
        Err(e) => return (false, e),
    };

    let base = Path::new(&home).join(".local");
    let lib_dir = base.join("lib/fcitx5");
    let addon_dir = base.join("share/fcitx5/addon");
    let im_dir = base.join("share/fcitx5/inputmethod");
    let app_dir = base.join("share/applications");

    for d in [&lib_dir, &addon_dir, &im_dir, &app_dir] {
        if let Err(e) = fs::create_dir_all(d) {
            return (false, format!("Không thể tạo thư mục {}: {e}", d.display()));
        }
    }

    if let Err(e) = fs::copy(&so_path, lib_dir.join("libfcitx5_tregai.so")) {
        return (false, format!("Lỗi sao chép libfcitx5_tregai.so: {e}"));
    }
    let _ = fs::write(addon_dir.join("tregai.conf"), ADDON_CONF_CONTENT);
    let _ = fs::write(im_dir.join("tregai.conf"), IM_CONF_CONTENT);
    let _ = fs::write(app_dir.join("tregai-gui.desktop"), DESKTOP_ENTRY_CONTENT);

    let _ = restart_fcitx5();
    (
        true,
        "Đã cài đặt Tre Gai vào ~/.local và khởi động lại Fcitx5 thành công!".to_string(),
    )
}

fn remove_user_local_files() {
    if let Ok(home) = env::var("HOME") {
        let base = Path::new(&home).join(".local");
        let _ = fs::remove_file(base.join("lib/fcitx5/libfcitx5_tregai.so"));
        let _ = fs::remove_file(base.join("share/fcitx5/addon/tregai.conf"));
        let _ = fs::remove_file(base.join("share/fcitx5/inputmethod/tregai.conf"));
        let _ = fs::remove_file(base.join("share/applications/tregai-gui.desktop"));
    }
}

fn install_system_wide() -> (bool, String) {
    let so_path = match locate_or_build_shared_library() {
        Ok(p) => p,
        Err(e) => return (false, e),
    };

    let sys_libdir = detect_system_fcitx5_libdir();
    let tmp_dir = env::temp_dir().join("fcitx5-tregai-install");
    let _ = fs::create_dir_all(&tmp_dir);

    let addon_tmp = tmp_dir.join("tregai-addon.conf");
    let im_tmp = tmp_dir.join("tregai.conf");
    let desktop_tmp = tmp_dir.join("tregai-gui.desktop");

    let _ = fs::write(&addon_tmp, ADDON_CONF_CONTENT);
    let _ = fs::write(&im_tmp, IM_CONF_CONTENT);
    let _ = fs::write(&desktop_tmp, DESKTOP_ENTRY_CONTENT);

    let script = format!(
        "install -Dm755 '{}' '{}/libfcitx5_tregai.so' && \
         install -Dm644 '{}' '/usr/share/fcitx5/addon/tregai.conf' && \
         install -Dm644 '{}' '/usr/share/fcitx5/inputmethod/tregai.conf' && \
         install -Dm644 '{}' '/usr/share/applications/tregai-gui.desktop'",
        so_path.display(),
        sys_libdir.display(),
        addon_tmp.display(),
        im_tmp.display(),
        desktop_tmp.display()
    );

    let status = Command::new("pkexec").args(["sh", "-c", &script]).status();

    match status {
        Ok(s) if s.success() => {
            remove_user_local_files();
            let _ = restart_fcitx5();
            (
                true,
                "Đã cài đặt Toàn Hệ Thống (/usr) và khởi động lại Fcitx5!".to_string(),
            )
        }
        _ => (
            false,
            "Cài đặt hệ thống bị hủy hoặc pkexec thất bại.".to_string(),
        ),
    }
}

fn uninstall_all() -> (bool, String) {
    remove_user_local_files();

    if is_system_installed() {
        let sys_so = detect_system_fcitx5_libdir().join("libfcitx5_tregai.so");
        let script = format!(
            "rm -f '{}' '/usr/share/fcitx5/addon/tregai.conf' '/usr/share/fcitx5/inputmethod/tregai.conf' '/usr/share/applications/tregai-gui.desktop'",
            sys_so.display()
        );
        let _ = Command::new("pkexec").args(["sh", "-c", &script]).status();
    }

    let _ = restart_fcitx5();
    (
        true,
        "Đã gỡ cài đặt bộ gõ Tre Gai và làm mới Fcitx5.".to_string(),
    )
}

fn notify_fcitx5_reload() {
    let _ = Command::new("fcitx5-remote")
        .arg("-r")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn restart_fcitx5() -> bool {
    Command::new("fcitx5")
        .args(["-r", "-d"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="vi">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Bộ gõ Tre Gai — Trình Quản Lý Cấu Hình Fcitx5</title>
<style>
:root {
  --bg: #0f172a;
  --card: #1e293b;
  --border: #334155;
  --text: #f8fafc;
  --muted: #94a3b8;
  --accent: #10b981;
  --accent-hover: #059669;
  --danger: #ef4444;
}
* { box-sizing: border-box; margin: 0; padding: 0; }
body {
  font-family: system-ui, -apple-system, sans-serif;
  background: var(--bg);
  color: var(--text);
  padding: 24px;
  line-height: 1.5;
}
.container { max-width: 940px; margin: 0 auto; }
header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 20px;
  padding-bottom: 16px;
  border-bottom: 1px solid var(--border);
}
h1 { font-size: 1.5rem; color: var(--accent); }
.badges span {
  display: inline-block;
  padding: 4px 10px;
  border-radius: 999px;
  font-size: 0.8rem;
  background: var(--card);
  border: 1px solid var(--border);
  margin-left: 8px;
}
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin-bottom: 16px; }
@media (max-width: 760px) { .grid { grid-template-columns: 1fr; } }
.card {
  background: var(--card);
  border: 1px solid var(--border);
  border-radius: 12px;
  padding: 18px;
}
.card h2 { font-size: 1.05rem; margin-bottom: 12px; color: #e2e8f0; }
.field { margin-bottom: 12px; }
.field label { display: block; font-size: 0.88rem; color: var(--muted); margin-bottom: 4px; }
select, input[type="text"], input[type="number"], textarea {
  width: 100%;
  padding: 8px 10px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: #0f172a;
  color: var(--text);
  font-size: 0.92rem;
}
.check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  font-size: 0.9rem;
}
.actions { display: flex; flex-wrap: wrap; gap: 10px; margin-top: 12px; }
button {
  padding: 9px 16px;
  border-radius: 8px;
  border: none;
  font-weight: 600;
  cursor: pointer;
  background: var(--accent);
  color: #06281e;
  transition: opacity 0.15s;
}
button:hover { background: var(--accent-hover); color: #fff; }
button.secondary { background: #334155; color: var(--text); }
button.danger { background: var(--danger); color: #fff; }
#toast {
  margin-top: 12px;
  padding: 10px 14px;
  border-radius: 8px;
  background: #064e3b;
  color: #a7f3d0;
  display: none;
}
</style>
</head>
<body>
<div class="container">
  <header>
    <div>
      <h1> Bộ Gõ Tre Gai (fcitx5-tregai)</h1>
      <p style="color:var(--muted);font-size:0.88rem;">Bộ gõ tiếng Việt tốc độ cao bằng Rust cho Fcitx5</p>
    </div>
    <div class="badges">
      <span id="badge-user">~/.local: Đang kiểm tra</span>
      <span id="badge-sys">/usr: Đang kiểm tra</span>
    </div>
  </header>

  <div class="card" style="margin-bottom:16px;">
    <h2>Cài Đặt & Điều Khiển Nhanh Fcitx5</h2>
    <div class="actions">
      <button type="button" onclick="postAction('/api/install-user')">Cài Đặt Cấp Người Dùng (~/.local)</button>
      <button type="button" class="secondary" onclick="postAction('/api/install-system')">Cài Đặt Toàn Hệ Thống (/usr)</button>
      <button type="button" class="secondary" onclick="postAction('/api/restart-fcitx5')">Khởi Động Lại Fcitx5</button>
      <button type="button" class="secondary" onclick="postAction('/api/open-configtool')">Mở fcitx5-configtool</button>
      <button type="button" class="danger" onclick="postAction('/api/uninstall')">Gỡ Cài Đặt</button>
    </div>
    <div id="toast"></div>
  </div>

  <form id="cfg-form" onsubmit="saveConfig(event)">
    <div class="grid">
      <div class="card">
        <h2>Phương Pháp Gõ & Chế Độ Xuất Chữ</h2>
        <label class="check-row"><input type="checkbox" id="enabled"> Bật xử lý tiếng Việt (Ctrl+Shift+S)</label>
        <div class="field">
          <label>Kiểu gõ</label>
          <select id="method">
            <option value="telex">Telex (Đầy đủ)</option>
            <option value="simple_telex">Simple Telex</option>
            <option value="vni">VNI</option>
            <option value="viqr">VIQR</option>
          </select>
        </div>
        <div class="field">
          <label>Bảng mã</label>
          <select id="charset">
            <option value="unicode">Unicode Dựng Sẵn (NFC)</option>
            <option value="unicode_composite">Unicode Tổ Hợp (NFD)</option>
          </select>
        </div>
        <div class="field">
          <label>Chế độ điều phối (Smooth Mode)</label>
          <select id="smooth_mode">
            <option value="ultra_smooth">Ultra Smooth (Client Preedit 0ms)</option>
            <option value="smooth">Smooth (SurroundingText tự động Fallback)</option>
            <option value="slow_compat">Slow Compat (Tương thích X11/Wine)</option>
            <option value="gaming">Gaming (Giữ nguyên phím WASD)</option>
          </select>
        </div>
        <div class="field">
          <label>Độ trễ tương thích Slow Compat (ms)</label>
          <input type="number" id="commit_delay_ms" min="0" max="100" value="15">
        </div>
      </div>

      <div class="card">
        <h2>Quy Tắc Chính Tả & Tốc Ký</h2>
        <label class="check-row"><input type="checkbox" id="underline_preedit"> Gạch chân từ đang gõ (Preedit)</label>
        <label class="check-row"><input type="checkbox" id="modern_tone"> Đặt dấu kiểu mới (hoà, khoẻ, thuỷ)</label>
        <label class="check-row"><input type="checkbox" id="spell_check"> Kiểm tra chính tả âm vị tiếng Việt</label>
        <label class="check-row"><input type="checkbox" id="auto_restore_english"> Tự động khôi phục từ tiếng Anh</label>
        <label class="check-row"><input type="checkbox" id="free_tone"> Cho phép bỏ dấu tự do ở cuối từ</label>
        <label class="check-row"><input type="checkbox" id="w_to_u"> Biến đổi phím W đầu từ thành Ư</label>
        <label class="check-row"><input type="checkbox" id="bracket_shortcuts"> Phím tắt ngoặc vuông [ ] thành ơ ư</label>
        <label class="check-row"><input type="checkbox" id="quick_start_consonant"> Gõ tắt phụ âm đầu (f->ph, j->gi, w->qu)</label>
        <label class="check-row"><input type="checkbox" id="quick_end_consonant"> Gõ tắt phụ âm cuối (g->ng, h->nh, k->ch)</label>
        <label class="check-row"><input type="checkbox" id="auto_capitalize"> Tự động viết hoa chữ cái đầu câu</label>
        <label class="check-row"><input type="checkbox" id="double_space_period"> Nhấn đúp phím Cách thành dấu chấm (. )</label>
        <label class="check-row"><input type="checkbox" id="allow_foreign_consonants"> Cho phép phụ âm ngoại lai (f, j, w, z)</label>
      </div>
    </div>

    <div class="grid">
      <div class="card">
        <h2>Bảng Gõ Tắt (Macros) & Ứng Dụng Loại Trừ</h2>
        <label class="check-row"><input type="checkbox" id="enable_macros"> Bật gõ tắt (Macros)</label>
        <label class="check-row"><input type="checkbox" id="macro_in_english_mode"> Mở rộng Macro ngay cả khi tắt tiếng Việt</label>
        <div class="field">
          <label>Ứng dụng loại trừ (cách nhau bằng dấu phẩy)</label>
          <input type="text" id="excluded_apps" placeholder="steam, cs2, dota2, lutris">
        </div>
        <div class="field">
          <label>Danh sách từ gõ tắt (mỗi dòng: viết_tắt = từ_đầy_đủ)</label>
          <textarea id="macros_text" rows="5"></textarea>
        </div>
        <button type="submit">💾 Lưu Cấu Hình & Áp Dụng Nóng</button>
      </div>

      <div class="card">
        <h2>Khung Kiểm Thử Trực Tiếp (Sandbox)</h2>
        <div class="field">
          <label>Nhập chuỗi phím thô (ví dụ: Booj gox Tre Gai sieeu muwowtj!)</label>
          <input type="text" id="sim-input" oninput="runSimulate()" placeholder="Gõ phím thử tại đây...">
        </div>
        <div class="field">
          <label>Kết quả đầu ra từ nhân Rust</label>
          <textarea id="sim-output" rows="6" readonly></textarea>
        </div>
      </div>
    </div>
  </form>
</div>

<script>
const BOOL_FIELDS = [
  'enabled','underline_preedit','modern_tone','spell_check',
  'auto_restore_english','free_tone','w_to_u','bracket_shortcuts',
  'quick_start_consonant','quick_end_consonant','auto_capitalize',
  'double_space_period','allow_foreign_consonants','enable_macros',
  'macro_in_english_mode'
];

function showToast(msg) {
  const el = document.getElementById('toast');
  el.textContent = msg;
  el.style.display = 'block';
}

async function loadState() {
  const res = await fetch('/api/state');
  const data = await res.json();
  BOOL_FIELDS.forEach(k => { document.getElementById(k).checked = !!data[k]; });
  document.getElementById('method').value = data.method;
  document.getElementById('charset').value = data.charset;
  document.getElementById('smooth_mode').value = data.smooth_mode;
  document.getElementById('commit_delay_ms').value = data.commit_delay_ms;
  document.getElementById('excluded_apps').value = data.excluded_apps;
  document.getElementById('macros_text').value = data.macros_text;
  document.getElementById('badge-user').textContent =
    data.user_installed ? '~/.local: Đã cài' : '~/.local: Chưa cài';
  document.getElementById('badge-sys').textContent =
    data.system_installed ? '/usr: Đã cài' : '/usr: Chưa cài';
}

async function saveConfig(e) {
  e.preventDefault();
  const params = new URLSearchParams();
  BOOL_FIELDS.forEach(k => {
    params.set(k, document.getElementById(k).checked ? 'true' : 'false');
  });
  params.set('method', document.getElementById('method').value);
  params.set('charset', document.getElementById('charset').value);
  params.set('smooth_mode', document.getElementById('smooth_mode').value);
  params.set('commit_delay_ms', document.getElementById('commit_delay_ms').value);
  params.set('excluded_apps', document.getElementById('excluded_apps').value);
  params.set('macros_text', document.getElementById('macros_text').value);

  const res = await fetch('/api/save', {
    method: 'POST',
    body: params.toString()
  });
  const data = await res.json();
  showToast(data.message);
  runSimulate();
}

async function postAction(endpoint) {
  showToast('Đang thực thi...');
  const res = await fetch(endpoint, { method: 'POST' });
  const data = await res.json();
  showToast(data.message);
  loadState();
}

async function runSimulate() {
  const params = new URLSearchParams();
  params.set('text', document.getElementById('sim-input').value);
  const res = await fetch('/api/simulate', {
    method: 'POST',
    body: params.toString()
  });
  const data = await res.json();
  document.getElementById('sim-output').value = data.output;
}

loadState();
</script>
</body>
</html>
"#;
