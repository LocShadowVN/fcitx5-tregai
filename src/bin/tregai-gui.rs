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
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const ADDON_CONF_CONTENT: &str = "[Addon]\n\
Name=Tre Gai Vietnamese Input Method\n\
Comment=Vietnamese Input Method Engine for Fcitx5 written in Rust\n\
Category=InputMethod\n\
Version=1.0.1\n\
Library=libfcitx5_tregai\n\
Type=SharedLibrary\n\
OnDemand=True\n\
Configurable=False\n";

const IM_CONF_CONTENT: &str = "[InputMethod]\n\
Name=Tiếng Việt (Tre Gai)\n\
Icon=input-keyboard\n\
Label=TG\n\
LangCode=vi\n\
Addon=tregai\n\
Configurable=False\n";

fn main() {
    let config = Arc::new(Mutex::new(ImeConfig::load_from_disk()));
    let listener =
        TcpListener::bind("127.0.0.1:0").expect("Failed to bind local configuration HTTP server");
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{}", port);

    println!("Tre Gai Configuration Server listening on {}", url);

    #[cfg(feature = "native-webview")]
    {
        let server_cfg = Arc::clone(&config);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let cfg_clone = Arc::clone(&server_cfg);
                thread::spawn(move || {
                    handle_http_client(stream, cfg_clone);
                });
            }
        });
        launch_native_webview(&url);
    }

    #[cfg(not(feature = "native-webview"))]
    {
        open_system_webview(&url);
        for stream in listener.incoming().flatten() {
            let cfg_clone = Arc::clone(&config);
            thread::spawn(move || {
                handle_http_client(stream, cfg_clone);
            });
        }
    }
}

#[cfg(feature = "native-webview")]
fn launch_native_webview(url: &str) {
    use tao::{
        dpi::LogicalSize,
        event::{Event, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        window::WindowBuilder,
    };
    use wry::WebViewBuilder;

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("Tre Gai Input Method Configuration")
        .with_inner_size(LogicalSize::new(1040.0, 800.0))
        .build(&event_loop)
        .expect("Failed to create window");

    let builder = WebViewBuilder::new().with_url(url);

    #[cfg(not(target_os = "linux"))]
    let _webview = builder.build(&window).expect("Failed to create WebView");

    #[cfg(target_os = "linux")]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().expect("Failed to acquire GTK VBox");
        builder
            .build_gtk(vbox)
            .expect("Failed to create WebKitGTK WebView")
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            std::process::exit(0);
        }
    });
}

#[cfg(not(feature = "native-webview"))]
fn open_system_webview(url: &str) {
    let app_arg = format!("--app={}", url);
    let browsers = [
        ("chromium", vec![app_arg.as_str(), "--new-window"]),
        ("google-chrome", vec![app_arg.as_str(), "--new-window"]),
        ("brave-browser", vec![app_arg.as_str(), "--new-window"]),
        ("microsoft-edge", vec![app_arg.as_str(), "--new-window"]),
        ("epiphany", vec![url]),
        ("xdg-open", vec![url]),
    ];

    for (bin, args) in browsers {
        if Command::new(bin).args(args).spawn().is_ok() {
            return;
        }
    }
    println!("Open the following URL in your browser: {}", url);
}

fn read_http_request(stream: &mut TcpStream) -> Option<(String, String)> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buffer = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    let mut header_end = None;
    let mut content_length = 0usize;

    loop {
        let Ok(n) = stream.read(&mut chunk) else {
            break;
        };
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);

        if header_end.is_none() {
            if let Some(pos) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                let end = pos + 4;
                header_end = Some(end);
                let header_str = String::from_utf8_lossy(&buffer[..end]);
                for line in header_str.lines() {
                    if let Some((k, v)) = line.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            content_length = v.trim().parse::<usize>().unwrap_or(0);
                        }
                    }
                }
            }
        }

        if let Some(end) = header_end {
            if buffer.len() >= end + content_length {
                break;
            }
        }
        if buffer.len() > 262_144 {
            break;
        }
    }

    let end = header_end?;
    let header_str = String::from_utf8_lossy(&buffer[..end]);
    let first_line = header_str.lines().next().unwrap_or("").to_string();
    let body_bytes = &buffer[end..buffer.len().min(end + content_length)];
    let body = String::from_utf8_lossy(body_bytes).into_owned();
    Some((first_line, body))
}

fn handle_http_client(mut stream: TcpStream, config: Arc<Mutex<ImeConfig>>) {
    let Some((first_line, body)) = read_http_request(&mut stream) else {
        return;
    };

    if first_line.starts_with("GET / ") || first_line.starts_with("GET /index.html") {
        send_response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            WEBVIEW_HTML,
        );
    } else if first_line.starts_with("GET /api/state") {
        let mut cfg = config.lock().unwrap();
        *cfg = ImeConfig::load_from_disk();
        let json = build_state_json(&cfg);
        send_response(&mut stream, "200 OK", "application/json", &json);
    } else if first_line.starts_with("POST /api/save") {
        let mut cfg = config.lock().unwrap();
        update_config_from_form(&mut cfg, &body);
        let save_ok = cfg.save_to_disk().is_ok();
        reload_fcitx5_dbus();
        let msg = if save_ok {
            format!(
                r#"{{"ok":true,"smooth_mode":"{}","message":"Đã lưu cấu hình (Chế độ: {})"}}"#,
                cfg.smooth_mode.as_str(),
                cfg.smooth_mode.as_str()
            )
        } else {
            r#"{"ok":false,"message":"Không thể ghi tệp cấu hình!"}"#.to_string()
        };
        send_response(&mut stream, "200 OK", "application/json", &msg);
    } else if first_line.starts_with("POST /api/test") {
        let cfg = config.lock().unwrap().clone();
        let raw_text = parse_form_field(&body, "text").unwrap_or_default();
        let mut engine = ImeEngine::new(cfg);
        let mut out = String::new();
        for ch in raw_text.chars() {
            let res = engine.process_char(ch);
            if let Some(c) = res.commit {
                out.push_str(&c);
            } else if !res.handled {
                out.push(ch);
            }
        }
        if let Some(rem) = engine.finalize_word() {
            out.push_str(&rem);
        }
        let resp = format!(r#"{{"result":"{}"}}"#, escape_json(&out));
        send_response(&mut stream, "200 OK", "application/json", &resp);
    } else if first_line.starts_with("POST /api/install-user") {
        let (ok, msg) = install_plugin_user_local();
        let resp = format!(r#"{{"ok":{},"message":"{}"}}"#, ok, escape_json(&msg));
        send_response(&mut stream, "200 OK", "application/json", &resp);
    } else if first_line.starts_with("POST /api/install-system") {
        let (ok, msg) = install_plugin_system_pkexec();
        let resp = format!(r#"{{"ok":{},"message":"{}"}}"#, ok, escape_json(&msg));
        send_response(&mut stream, "200 OK", "application/json", &resp);
    } else if first_line.starts_with("POST /api/restart-fcitx5") {
        let _ = Command::new("fcitx5").args(["-r", "-d"]).spawn();
        reload_fcitx5_dbus();
        send_response(
            &mut stream,
            "200 OK",
            "application/json",
            r#"{"ok":true,"message":"Đã khởi động lại Fcitx5."}"#,
        );
    } else if first_line.starts_with("POST /api/uninstall") {
        let msg = uninstall_plugin_user_local();
        let resp = format!(r#"{{"ok":true,"message":"{}"}}"#, escape_json(&msg));
        send_response(&mut stream, "200 OK", "application/json", &resp);
    } else {
        send_response(&mut stream, "404 Not Found", "text/plain", "Not Found");
    }
}

fn send_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        status,
        content_type,
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

fn check_installation_status() -> (bool, bool, bool) {
    let home = env::var("HOME").unwrap_or_default();
    let user_so = PathBuf::from(&home).join(".local/lib/fcitx5/libfcitx5_tregai.so");
    let sys_so_usr = PathBuf::from("/usr/lib/fcitx5/libfcitx5_tregai.so");
    let sys_so_64 = PathBuf::from("/usr/lib64/fcitx5/libfcitx5_tregai.so");
    let sys_so_multi = PathBuf::from("/usr/lib/x86_64-linux-gnu/fcitx5/libfcitx5_tregai.so");

    let user_installed = user_so.exists();
    let sys_installed = sys_so_usr.exists() || sys_so_64.exists() || sys_so_multi.exists();
    let fcitx_running = Command::new("pgrep")
        .arg("-x")
        .arg("fcitx5")
        .output()
        .is_ok_and(|o| o.status.success());

    (user_installed, sys_installed, fcitx_running)
}

fn locate_or_build_plugin_so() -> Result<PathBuf, String> {
    if Path::new("Cargo.toml").exists() {
        let status = Command::new("cargo")
            .args(["build", "--release", "--features", "fcitx5-plugin"])
            .status();
        if status.is_ok_and(|s| s.success()) {
            let built = PathBuf::from("target/release/libfcitx5_tregai.so");
            if built.exists() {
                return Ok(built);
            }
        }
    }

    let mut candidates = vec![
        PathBuf::from("target/release/libfcitx5_tregai.so"),
        PathBuf::from("libfcitx5_tregai.so"),
        PathBuf::from("/usr/lib/x86_64-linux-gnu/fcitx5/libfcitx5_tregai.so"),
        PathBuf::from("/usr/lib/fcitx5/libfcitx5_tregai.so"),
        PathBuf::from("/usr/lib64/fcitx5/libfcitx5_tregai.so"),
    ];

    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.insert(0, parent.join("libfcitx5_tregai.so"));
        }
    }

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(
        "Không tìm thấy libfcitx5_tregai.so đã biên dịch hoặc thư mục hiện tại không có Cargo.toml."
            .to_string(),
    )
}

fn install_plugin_user_local() -> (bool, String) {
    let so_source = match locate_or_build_plugin_so() {
        Ok(p) => p,
        Err(e) => return (false, e),
    };

    let Ok(home) = env::var("HOME") else {
        return (false, "HOME environment variable is not set.".to_string());
    };
    let home_path = PathBuf::from(&home);
    let addon_lib_dir = home_path.join(".local/lib/fcitx5");
    let addon_conf_dir = home_path.join(".local/share/fcitx5/addon");
    let im_conf_dir = home_path.join(".local/share/fcitx5/inputmethod");
    let app_dir = home_path.join(".local/share/applications");
    let bin_dir = home_path.join(".local/bin");

    for dir in [
        &addon_lib_dir,
        &addon_conf_dir,
        &im_conf_dir,
        &app_dir,
        &bin_dir,
    ] {
        if let Err(e) = fs::create_dir_all(dir) {
            return (false, format!("Failed to create directory: {}", e));
        }
    }

    let target_so = addon_lib_dir.join("libfcitx5_tregai.so");
    if so_source != target_so {
        if let Err(e) = fs::copy(&so_source, &target_so) {
            return (false, format!("Failed to copy shared library: {}", e));
        }
    }

    let _ = fs::write(addon_conf_dir.join("tregai.conf"), ADDON_CONF_CONTENT);
    let _ = fs::write(im_conf_dir.join("tregai.conf"), IM_CONF_CONTENT);

    if let Ok(exe) = env::current_exe() {
        let target_gui = bin_dir.join("tregai-gui");
        if exe != target_gui {
            let _ = fs::copy(&exe, &target_gui);
        }
        let desktop_content = format!(
            "[Desktop Entry]\nName=Tre Gai Input Method\nComment=Configure Tre Gai Vietnamese Input Method for Fcitx5\nExec={}\nIcon=input-keyboard\nTerminal=false\nType=Application\nCategories=Settings;Utility;\n",
            target_gui.display()
        );
        let _ = fs::write(app_dir.join("tregai-gui.desktop"), desktop_content);
    }

    let _ = Command::new("fcitx5").args(["-r", "-d"]).spawn();
    (
        true,
        "Đã cài đặt vào ~/.local và khởi động lại Fcitx5.".to_string(),
    )
}

fn install_plugin_system_pkexec() -> (bool, String) {
    let so_source = match locate_or_build_plugin_so() {
        Ok(p) => p,
        Err(e) => return (false, e),
    };

    let libdir = Command::new("pkg-config")
        .args(["--variable=libdir", "Fcitx5Core"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if Path::new("/usr/lib/x86_64-linux-gnu/fcitx5").exists() {
                "/usr/lib/x86_64-linux-gnu".to_string()
            } else {
                "/usr/lib".to_string()
            }
        });

    let tmp_addon = "/tmp/tregai-addon.conf";
    let tmp_im = "/tmp/tregai.conf";
    let _ = fs::write(tmp_addon, ADDON_CONF_CONTENT);
    let _ = fs::write(tmp_im, IM_CONF_CONTENT);

    let cmd = format!(
        "install -Dm755 '{}' '{}/fcitx5/libfcitx5_tregai.so' && \
         install -Dm644 '{}' /usr/share/fcitx5/addon/tregai.conf && \
         install -Dm644 '{}' /usr/share/fcitx5/inputmethod/tregai.conf",
        so_source.display(),
        libdir,
        tmp_addon,
        tmp_im
    );

    let pk = Command::new("pkexec").args(["sh", "-c", &cmd]).status();
    if pk.is_ok_and(|s| s.success()) {
        let _ = Command::new("fcitx5").args(["-r", "-d"]).spawn();
        (
            true,
            format!(
                "Đã cài đặt toàn hệ thống tại {}/fcitx5 và khởi động lại Fcitx5.",
                libdir
            ),
        )
    } else {
        (false, "Cài đặt hệ thống bị hủy hoặc thất bại.".to_string())
    }
}

fn uninstall_plugin_user_local() -> String {
    if let Ok(home) = env::var("HOME") {
        let h = PathBuf::from(home);
        let _ = fs::remove_file(h.join(".local/lib/fcitx5/libfcitx5_tregai.so"));
        let _ = fs::remove_file(h.join(".local/share/fcitx5/addon/tregai.conf"));
        let _ = fs::remove_file(h.join(".local/share/fcitx5/inputmethod/tregai.conf"));
        let _ = fs::remove_file(h.join(".local/share/applications/tregai-gui.desktop"));
    }
    let _ = Command::new("fcitx5").args(["-r", "-d"]).spawn();
    "Đã gỡ bản cài đặt tại ~/.local.".to_string()
}

fn reload_fcitx5_dbus() {
    let _ = Command::new("dbus-send")
        .args([
            "--session",
            "--type=method_call",
            "--dest=org.fcitx.Fcitx5",
            "/controller",
            "org.fcitx.Fcitx.Controller1.ReloadAddonConfig",
            "string:tregai",
        ])
        .output();
}

fn escape_json(s: &str) -> String {
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

fn build_state_json(cfg: &ImeConfig) -> String {
    let (user_installed, sys_installed, fcitx_running) = check_installation_status();
    let mut macros_json = String::from("[");
    for (idx, (k, v)) in cfg.macros.iter().enumerate() {
        if idx > 0 {
            macros_json.push(',');
        }
        macros_json.push_str(&format!(
            r#"{{"key":"{}","val":"{}"}}"#,
            escape_json(k),
            escape_json(v)
        ));
    }
    macros_json.push(']');

    format!(
        r#"{{
            "enabled": {},
            "method": "{}",
            "charset": "{}",
            "smooth_mode": "{}",
            "commit_delay_ms": {},
            "underline_preedit": {},
            "modern_tone": {},
            "spell_check": {},
            "auto_restore_english": {},
            "free_tone": {},
            "w_to_u": {},
            "bracket_shortcuts": {},
            "quick_start_consonant": {},
            "quick_end_consonant": {},
            "auto_capitalize": {},
            "double_space_period": {},
            "allow_foreign_consonants": {},
            "enable_macros": {},
            "macro_in_english_mode": {},
            "excluded_apps": "{}",
            "macros": {},
            "status": {{
                "user_installed": {},
                "sys_installed": {},
                "fcitx_running": {}
            }}
        }}"#,
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
        escape_json(&cfg.excluded_apps.join(", ")),
        macros_json,
        user_installed,
        sys_installed,
        fcitx_running
    )
}

fn url_decode(input: &str) -> String {
    let raw = input.as_bytes();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let b = raw[i];
        if b == b'+' {
            bytes.push(b' ');
            i += 1;
        } else if b == b'%' && i + 2 < raw.len() {
            let hex = [raw[i + 1], raw[i + 2]];
            if let Ok(s) = std::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    bytes.push(val);
                }
            }
            i += 3;
        } else {
            bytes.push(b);
            i += 1;
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn parse_form_field(body: &str, target_key: &str) -> Option<String> {
    for pair in body.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if url_decode(k) == target_key {
            return Some(url_decode(v));
        }
    }
    None
}

fn update_config_from_form(cfg: &mut ImeConfig, body: &str) {
    let get = |k: &str| parse_form_field(body, k);
    let get_bool = |k: &str| get(k).is_some_and(|v| v == "true" || v == "1");

    if get("method").is_none() && get("smooth_mode").is_none() {
        return;
    }

    cfg.enabled = get_bool("enabled");
    if let Some(m) = get("method") {
        cfg.method = InputMethod::parse(&m);
    }
    if let Some(cs) = get("charset") {
        cfg.charset = Charset::parse(&cs);
    }
    if let Some(sm) = get("smooth_mode") {
        cfg.smooth_mode = SmoothMode::parse(&sm);
    }
    if let Some(delay) = get("commit_delay_ms") {
        cfg.commit_delay_ms = delay.parse::<u32>().unwrap_or(15).min(100);
    }
    cfg.underline_preedit = get_bool("underline_preedit");
    cfg.modern_tone = get_bool("modern_tone");
    cfg.spell_check = get_bool("spell_check");
    cfg.auto_restore_english = get_bool("auto_restore_english");
    cfg.free_tone = get_bool("free_tone");
    cfg.w_to_u = get_bool("w_to_u");
    cfg.bracket_shortcuts = get_bool("bracket_shortcuts");
    cfg.quick_start_consonant = get_bool("quick_start_consonant");
    cfg.quick_end_consonant = get_bool("quick_end_consonant");
    cfg.auto_capitalize = get_bool("auto_capitalize");
    cfg.double_space_period = get_bool("double_space_period");
    cfg.allow_foreign_consonants = get_bool("allow_foreign_consonants");
    cfg.enable_macros = get_bool("enable_macros");
    cfg.macro_in_english_mode = get_bool("macro_in_english_mode");

    if let Some(apps) = get("excluded_apps") {
        cfg.excluded_apps = apps
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
    }

    if let Some(macros_raw) = get("macros_text") {
        let mut map = BTreeMap::new();
        for line in macros_raw.lines() {
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim().to_lowercase();
                let val = v.trim().to_string();
                if !key.is_empty() && !val.is_empty() {
                    map.insert(key, val);
                }
            }
        }
        cfg.macros = map;
    }
}

const WEBVIEW_HTML: &str = r#####"<!DOCTYPE html>
<html lang="vi">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Bộ gõ Tre Gai — Cấu hình Hệ thống</title>
<style>
  :root {
    --bg: #09090b;
    --panel: #121215;
    --border: #27272a;
    --border-focus: #52525b;
    --text: #f4f4f5;
    --muted: #a1a1aa;
    --accent: #2563eb;
    --accent-hover: #1d4ed8;
    --success: #16a34a;
    --danger: #dc2626;
  }
  * { box-sizing: border-box; margin: 0; padding: 0; font-family: Inter, system-ui, -apple-system, sans-serif; }
  body { background: var(--bg); color: var(--text); padding: 28px; font-size: 14px; line-height: 1.5; }
  .layout { max-width: 960px; margin: 0 auto; display: flex; flex-direction: column; gap: 20px; }
  .topbar {
    display: flex; justify-content: space-between; align-items: center;
    padding-bottom: 18px; border-bottom: 1px solid var(--border);
  }
  .topbar h1 { font-size: 18px; font-weight: 600; letter-spacing: -0.01em; }
  .topbar p { font-size: 12px; color: var(--muted); margin-top: 2px; }
  .status-bar { display: flex; gap: 12px; align-items: center; }
  .status-pill {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 5px 12px; border-radius: 6px; font-size: 12px; font-weight: 500;
    background: var(--panel); border: 1px solid var(--border); color: var(--muted);
  }
  .dot { width: 8px; height: 8px; border-radius: 50%; background: #52525b; }
  .dot.active { background: var(--success); }
  .section {
    background: var(--panel); border: 1px solid var(--border);
    border-radius: 8px; padding: 20px;
  }
  .section-header {
    display: flex; justify-content: space-between; align-items: center;
    margin-bottom: 16px; padding-bottom: 10px; border-bottom: 1px solid var(--border);
  }
  .section-title { font-size: 13px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.04em; color: var(--muted); }
  .btn-group { display: flex; gap: 8px; flex-wrap: wrap; }
  button {
    cursor: pointer; border-radius: 6px; padding: 8px 14px;
    font-size: 13px; font-weight: 500; border: 1px solid transparent;
    transition: background 0.12s, border-color 0.12s;
  }
  .btn-primary { background: var(--accent); color: #fff; }
  .btn-primary:hover { background: var(--accent-hover); }
  .btn-secondary { background: #18181b; color: var(--text); border-color: var(--border); }
  .btn-secondary:hover { border-color: var(--border-focus); background: #27272a; }
  .btn-danger { background: transparent; color: #f87171; border-color: rgba(220,38,38,0.4); }
  .btn-danger:hover { background: rgba(220,38,38,0.1); }
  .mode-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 10px; }
  @media (max-width: 760px) { .mode-grid { grid-template-columns: repeat(2, 1fr); } }
  .mode-option {
    border: 1px solid var(--border); border-radius: 6px; padding: 12px;
    cursor: pointer; background: #09090b; transition: border-color 0.12s;
  }
  .mode-option.selected { border-color: var(--accent); background: rgba(37, 99, 235, 0.12); }
  .mode-option h3 { font-size: 13px; font-weight: 600; margin-bottom: 4px; color: var(--text); }
  .mode-option p { font-size: 12px; color: var(--muted); line-height: 1.4; }
  .options-grid { display: grid; grid-template-columns: repeat(2, 1fr); gap: 10px; }
  @media (max-width: 680px) { .options-grid { grid-template-columns: 1fr; } }
  .option-row {
    display: flex; justify-content: space-between; align-items: center;
    padding: 10px 12px; background: #09090b; border: 1px solid var(--border); border-radius: 6px;
  }
  .option-row .label { font-size: 13px; font-weight: 500; }
  .option-row .desc { font-size: 11px; color: var(--muted); display: block; margin-top: 2px; }
  input[type="checkbox"] { width: 16px; height: 16px; accent-color: var(--accent); cursor: pointer; }
  select, input[type="text"], textarea {
    width: 100%; background: #09090b; color: var(--text); border: 1px solid var(--border);
    border-radius: 6px; padding: 8px 12px; font-size: 13px; outline: none;
  }
  select:focus, input[type="text"]:focus, textarea:focus { border-color: var(--accent); }
  .control-bar { display: grid; grid-template-columns: repeat(2, 1fr); gap: 12px; margin-bottom: 14px; }
  .slider-row {
    margin-top: 12px; padding: 10px 12px; background: #09090b;
    border: 1px solid var(--border); border-radius: 6px;
  }
  .notification {
    position: fixed; bottom: 20px; right: 20px; padding: 10px 16px; border-radius: 6px;
    background: #18181b; color: var(--text); font-size: 13px; font-weight: 500;
    border: 1px solid var(--border-focus); display: none; z-index: 100;
  }
</style>
</head>
<body>
<div class="layout">
  <div class="topbar">
    <div>
      <h1>Bộ gõ Tre Gai (fcitx5-tregai)</h1>
      <p>Vietnamese Input Method Engine for Fcitx5 • GPL-3.0</p>
    </div>
    <div class="status-bar">
      <div class="status-pill"><span id="dot-install" class="dot"></span><span id="text-install">Checking...</span></div>
      <div class="status-pill"><span id="dot-fcitx" class="dot"></span><span id="text-fcitx">Fcitx5</span></div>
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <span class="section-title">Quản lý Cài đặt Hệ thống</span>
    </div>
    <div class="btn-group">
      <button class="btn-primary" onclick="apiAction('/api/install-user')">Cài đặt Cấp Người Dùng (~/.local)</button>
      <button class="btn-secondary" onclick="apiAction('/api/install-system')">Cài đặt Toàn Hệ Thống (/usr)</button>
      <button class="btn-secondary" onclick="apiAction('/api/restart-fcitx5')">Khởi động lại Fcitx5</button>
      <button class="btn-danger" onclick="apiAction('/api/uninstall')">Gỡ bản cài ~/.local</button>
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <span class="section-title">Chế độ Xuất chữ & Tốc độ Phản hồi (Bấm chọn để áp dụng ngay)</span>
    </div>
    <div class="mode-grid" id="smooth-cards">
      <div class="mode-option" data-mode="ultra_smooth" onclick="changeSmoothMode('ultra_smooth')">
        <h3>Ultra Smooth (Preedit)</h3>
        <p>Phản hồi 0ms qua Client Preedit. An toàn tuyệt đối cho Wayland, GTK4 và Qt6.</p>
      </div>
      <div class="mode-option" data-mode="smooth" onclick="changeSmoothMode('smooth')">
        <h3>Direct (Lotus / Backspace)</h3>
        <p>Gõ trực tiếp không dùng Preedit qua Surrounding Text & Minimal Diff Backspace.</p>
      </div>
      <div class="mode-option" data-mode="slow_compat" onclick="changeSmoothMode('slow_compat')">
        <h3>Compatibility Delay</h3>
        <p>Chế độ Direct kèm nhịp trễ chống nuốt chữ cho X11, Electron cũ hoặc Wine.</p>
      </div>
      <div class="mode-option" data-mode="gaming" onclick="changeSmoothMode('gaming')">
        <h3>Gaming Mode</h3>
        <p>Tắt biến đổi phím W đơn lẻ ở đầu từ để bảo toàn cụm phím di chuyển WASD.</p>
      </div>
    </div>

    <div class="slider-row">
      <div style="display:flex; justify-content:space-between; font-size:12px; color:var(--muted);">
        <span>Độ trễ xuất chữ (chỉ áp dụng cho chế độ Compatibility Delay)</span>
        <strong id="delay-label" style="color:var(--text);">15 ms</strong>
      </div>
      <input type="range" id="commit_delay_ms" min="0" max="100" value="15" style="width:100%; margin-top:6px;"
             oninput="document.getElementById('delay-label').innerText = this.value + ' ms'"
             onchange="saveConfig()">
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <span class="section-title">Kiểu gõ, Bảng mã & Quy tắc Ngôn ngữ (Chuẩn Lotus)</span>
      <button class="btn-primary" onclick="saveConfig()">Lưu & Áp dụng</button>
    </div>

    <div class="control-bar">
      <div>
        <label style="font-size:12px; color:var(--muted); display:block; margin-bottom:4px;">Phương pháp nhập liệu</label>
        <select id="method" onchange="saveConfig()">
          <option value="telex">Telex</option>
          <option value="simple_telex">Simple Telex</option>
          <option value="vni">VNI</option>
          <option value="viqr">VIQR</option>
        </select>
      </div>
      <div>
        <label style="font-size:12px; color:var(--muted); display:block; margin-bottom:4px;">Bảng mã đầu ra (Charset)</label>
        <select id="charset" onchange="saveConfig()">
          <option value="unicode">Unicode (UTF-8 Dựng sẵn - NFC)</option>
          <option value="unicode_composite">Unicode Tổ hợp (NFD)</option>
        </select>
      </div>
    </div>

    <div class="options-grid" onchange="saveConfig()">
      <div class="option-row">
        <div><span class="label">Bật bộ gõ Tiếng Việt</span><span class="desc">Phím tắt chuyển nhanh: Ctrl + Shift + S</span></div>
        <input type="checkbox" id="enabled">
      </div>
      <div class="option-row">
        <div><span class="label">Hiển thị gạch chân Preedit</span><span class="desc">Gạch chân từ đang soạn trong chế độ Preedit</span></div>
        <input type="checkbox" id="underline_preedit">
      </div>
      <div class="option-row">
        <div><span class="label">Đặt dấu kiểu mới (hoà, khoẻ, thuỷ)</span><span class="desc">Tắt để dùng quy tắc cũ (hòa, khỏe, thủy)</span></div>
        <input type="checkbox" id="modern_tone">
      </div>
      <div class="option-row">
        <div><span class="label">Kiểm tra chính tả Tiếng Việt</span><span class="desc">Nhận diện cấu trúc âm tiết hợp lệ</span></div>
        <input type="checkbox" id="spell_check">
      </div>
      <div class="option-row">
        <div><span class="label">Tự động khôi phục từ Tiếng Anh</span><span class="desc">Hoàn trả phím gốc khi âm tiết sai chính tả</span></div>
        <input type="checkbox" id="auto_restore_english">
      </div>
      <div class="option-row">
        <div><span class="label">Bỏ dấu tự do</span><span class="desc">Cho phép nhập dấu thanh ở bất kỳ vị trí nào trong từ</span></div>
        <input type="checkbox" id="free_tone">
      </div>
      <div class="option-row">
        <div><span class="label">Biến đổi W thành Ư ở đầu âm tiết</span><span class="desc">Hỗ trợ gõ w → ư, nhw → như trong Telex</span></div>
        <input type="checkbox" id="w_to_u">
      </div>
      <div class="option-row">
        <div><span class="label">Phím tắt ngoặc vuông ([ → ơ, ] → ư)</span><span class="desc">Áp dụng cho phương pháp gõ Telex</span></div>
        <input type="checkbox" id="bracket_shortcuts">
      </div>
      <div class="option-row">
        <div><span class="label">Gõ tắt phụ âm đầu (f→ph, j→gi, w→qu)</span><span class="desc">Mở rộng khi theo sau là nguyên âm</span></div>
        <input type="checkbox" id="quick_start_consonant">
      </div>
      <div class="option-row">
        <div><span class="label">Gõ tắt phụ âm cuối (g→ng, h→nh, k→ch)</span><span class="desc">Mở rộng khi đứng ngay sau nguyên âm</span></div>
        <input type="checkbox" id="quick_end_consonant">
      </div>
      <div class="option-row">
        <div><span class="label">Tự động viết hoa đầu câu</span><span class="desc">Viết hoa ký tự đầu sau dấu chấm câu '.', '!', '?'</span></div>
        <input type="checkbox" id="auto_capitalize">
      </div>
      <div class="option-row">
        <div><span class="label">Nhấn đúp phím Cách thành dấu chấm</span><span class="desc">Gõ 2 lần Space liên tiếp để xuất '. '</span></div>
        <input type="checkbox" id="double_space_period">
      </div>
      <div class="option-row">
        <div><span class="label">Cho phép phụ âm ngoại lai (z, w, j, f)</span><span class="desc">Chấp nhận phụ âm đầu ngoài bảng chữ cái chuẩn</span></div>
        <input type="checkbox" id="allow_foreign_consonants">
      </div>
      <div class="option-row">
        <div><span class="label">Kích hoạt bảng gõ tắt (Macros)</span><span class="desc">Mở rộng từ viết tắt khi kết thúc từ</span></div>
        <input type="checkbox" id="enable_macros">
      </div>
      <div class="option-row">
        <div><span class="label">Cho phép gõ tắt cả khi tắt Tiếng Việt</span><span class="desc">Hoạt động giống chế độ Macro in English của Lotus</span></div>
        <input type="checkbox" id="macro_in_english_mode">
      </div>
    </div>

    <div style="margin-top: 14px;">
      <label style="font-size:12px; color:var(--muted); display:block; margin-bottom:4px;">Danh sách ứng dụng tự động bỏ qua / tắt bộ gõ (phân cách bằng dấu phẩy)</label>
      <input type="text" id="excluded_apps" placeholder="steam, cs2, dota2, lutris" onchange="saveConfig()">
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <span class="section-title">Kiểm tra Trực tiếp Nhân Xử lý (Engine Sandbox)</span>
    </div>
    <div class="options-grid">
      <div>
        <label style="font-size:12px; color:var(--muted); display:block; margin-bottom:4px;">Chuỗi phím đầu vào (Raw Keystrokes)</label>
        <input type="text" id="sandbox-in" value="Booj gox Tre Gai sieeu muwowtj treen Linux." oninput="runSandboxTest()">
      </div>
      <div>
        <label style="font-size:12px; color:var(--muted); display:block; margin-bottom:4px;">Kết quả đầu ra (Engine Output)</label>
        <input type="text" id="sandbox-out" readonly>
      </div>
    </div>
  </div>

  <div class="section">
    <div class="section-header">
      <span class="section-title">Bảng Từ Điển Gõ Tắt (Cú pháp: từ_viết_tắt = cụm_từ_mở_rộng)</span>
      <button class="btn-primary" onclick="saveConfig()">Lưu Bảng Gõ Tắt</button>
    </div>
    <textarea id="macros_text" rows="6" spellcheck="false"></textarea>
  </div>
</div>

<div id="notification" class="notification"></div>

<script>
let currentSmoothMode = 'ultra_smooth';

function notify(msg) {
  const el = document.getElementById('notification');
  el.innerText = msg;
  el.style.display = 'block';
  setTimeout(() => { el.style.display = 'none'; }, 2800);
}

function selectSmoothMode(mode) {
  currentSmoothMode = mode;
  document.querySelectorAll('.mode-option').forEach(el => {
    el.classList.toggle('selected', el.getAttribute('data-mode') === mode);
  });
}

async function changeSmoothMode(mode) {
  selectSmoothMode(mode);
  await saveConfig();
}

async function loadState() {
  const res = await fetch('/api/state');
  const data = await res.json();

  document.getElementById('enabled').checked = data.enabled;
  document.getElementById('method').value = data.method;
  document.getElementById('charset').value = data.charset;
  selectSmoothMode(data.smooth_mode);
  document.getElementById('commit_delay_ms').value = data.commit_delay_ms;
  document.getElementById('delay-label').innerText = data.commit_delay_ms + ' ms';
  document.getElementById('underline_preedit').checked = data.underline_preedit;
  document.getElementById('modern_tone').checked = data.modern_tone;
  document.getElementById('spell_check').checked = data.spell_check;
  document.getElementById('auto_restore_english').checked = data.auto_restore_english;
  document.getElementById('free_tone').checked = data.free_tone;
  document.getElementById('w_to_u').checked = data.w_to_u;
  document.getElementById('bracket_shortcuts').checked = data.bracket_shortcuts;
  document.getElementById('quick_start_consonant').checked = data.quick_start_consonant;
  document.getElementById('quick_end_consonant').checked = data.quick_end_consonant;
  document.getElementById('auto_capitalize').checked = data.auto_capitalize;
  document.getElementById('double_space_period').checked = data.double_space_period;
  document.getElementById('allow_foreign_consonants').checked = data.allow_foreign_consonants;
  document.getElementById('enable_macros').checked = data.enable_macros;
  document.getElementById('macro_in_english_mode').checked = data.macro_in_english_mode;
  document.getElementById('excluded_apps').value = data.excluded_apps;

  document.getElementById('macros_text').value = data.macros.map(m => `${m.key} = ${m.val}`).join('\n');

  const dotInstall = document.getElementById('dot-install');
  const textInstall = document.getElementById('text-install');
  if (data.status.sys_installed) {
    dotInstall.className = 'dot active';
    textInstall.innerText = 'System (/usr)';
  } else if (data.status.user_installed) {
    dotInstall.className = 'dot active';
    textInstall.innerText = 'User (~/.local)';
  } else {
    dotInstall.className = 'dot';
    textInstall.innerText = 'Not Installed';
  }

  const dotFcitx = document.getElementById('dot-fcitx');
  const textFcitx = document.getElementById('text-fcitx');
  dotFcitx.className = data.status.fcitx_running ? 'dot active' : 'dot';
  textFcitx.innerText = data.status.fcitx_running ? 'Fcitx5 Active' : 'Fcitx5 Inactive';

  runSandboxTest();
}

async function saveConfig() {
  const params = new URLSearchParams();
  params.set('enabled', document.getElementById('enabled').checked);
  params.set('method', document.getElementById('method').value);
  params.set('charset', document.getElementById('charset').value);
  params.set('smooth_mode', currentSmoothMode);
  params.set('commit_delay_ms', document.getElementById('commit_delay_ms').value);
  params.set('underline_preedit', document.getElementById('underline_preedit').checked);
  params.set('modern_tone', document.getElementById('modern_tone').checked);
  params.set('spell_check', document.getElementById('spell_check').checked);
  params.set('auto_restore_english', document.getElementById('auto_restore_english').checked);
  params.set('free_tone', document.getElementById('free_tone').checked);
  params.set('w_to_u', document.getElementById('w_to_u').checked);
  params.set('bracket_shortcuts', document.getElementById('bracket_shortcuts').checked);
  params.set('quick_start_consonant', document.getElementById('quick_start_consonant').checked);
  params.set('quick_end_consonant', document.getElementById('quick_end_consonant').checked);
  params.set('auto_capitalize', document.getElementById('auto_capitalize').checked);
  params.set('double_space_period', document.getElementById('double_space_period').checked);
  params.set('allow_foreign_consonants', document.getElementById('allow_foreign_consonants').checked);
  params.set('enable_macros', document.getElementById('enable_macros').checked);
  params.set('macro_in_english_mode', document.getElementById('macro_in_english_mode').checked);
  params.set('excluded_apps', document.getElementById('excluded_apps').value);
  params.set('macros_text', document.getElementById('macros_text').value);

  const res = await fetch('/api/save', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded;charset=UTF-8' },
    body: params.toString()
  });
  const data = await res.json();
  if (data.smooth_mode) {
    selectSmoothMode(data.smooth_mode);
  }
  notify(data.message);
  runSandboxTest();
}

async function runSandboxTest() {
  const params = new URLSearchParams();
  params.set('text', document.getElementById('sandbox-in').value);
  const res = await fetch('/api/test', {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded;charset=UTF-8' },
    body: params.toString()
  });
  const data = await res.json();
  document.getElementById('sandbox-out').value = data.result;
}

async function apiAction(endpoint) {
  notify('Processing request...');
  const res = await fetch(endpoint, { method: 'POST' });
  const data = await res.json();
  notify(data.message);
  await loadState();
}

window.addEventListener('DOMContentLoaded', loadState);
</script>
</body>
</html>
"#####;
