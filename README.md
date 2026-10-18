# fcitx5-tregai

`fcitx5-tregai` (Bộ gõ Tre Gai) là bộ gõ tiếng Việt dành cho Fcitx5 trên Linux, được phát triển độc lập bằng Rust từ con số 0 và đi kèm trình quản lý cấu hình đồ họa WebView (`tregai-gui`).

Dự án được phân phối theo giấy phép **GNU General Public License v3.0 (GPL-3.0-or-later)**.

## Kiến trúc hệ thống

- `src/unicode.rs`: Bảng ánh xạ ký tự tiếng Việt sang Unicode dựng sẵn (NFC) với độ phức tạp O(1).
- `src/syllable.rs`: Bộ phân tích cấu trúc âm tiết tiếng Việt (phụ âm đầu, nguyên âm, phụ âm cuối), xử lý ngoại lệ ngữ âm (`qu`, `gi`, `ươ`, `uơ`, `oă`), định vị dấu thanh và kiểm tra âm vị học tiếng Việt.
- `src/engine.rs`: Máy trạng thái xử lý bốn phương pháp nhập liệu (`Telex`, `Simple Telex`, `VNI`, `VIQR`), hoàn tác phím lặp, gõ tắt phụ âm đầu/cuối và tự động khôi phục từ tiếng Anh.
- `src/config.rs`: Trình đọc/ghi cấu hình chuẩn INI (`$XDG_CONFIG_HOME/fcitx5-tregai/config.ini`) và bộ mở rộng từ gõ tắt (Macros) bảo toàn kiểu chữ hoa/thường.
- `src/ffi.rs` & `csrc/fcitx5_bridge.cpp`: Giao diện C-ABI kết nối `fcitx::InputMethodEngineV2` với nhân Rust, hỗ trợ tự động nạp nóng cấu hình (hot-reload) mà không cần khởi động lại phiên làm việc.
- `src/bin/tregai-gui.rs`: Ứng dụng giao diện WebView phục vụ cài đặt, gỡ cài đặt, cấu hình thông số và kiểm thử trực tiếp.

## Tính năng chính

1. **Phương pháp gõ**: Telex, Simple Telex, VNI, VIQR.
2. **Bốn chế độ điều phối xuất chữ (`smooth_mode`)**:
   - `ultra_smooth`: Phản hồi tức thì (0ms) qua Client Preedit, tối ưu cho Wayland, GTK4 và Qt6.
   - `smooth`: Xuất chữ trực tiếp không gạch chân thông qua `SurroundingText` của Fcitx5 (tự động chuyển về Preedit không gạch chân nếu ứng dụng đích không hỗ trợ `SurroundingText`).
   - `slow_compat`: Bổ sung độ trễ vi mô (`commit_delay_ms`, cấu hình từ `0ms` đến `100ms`) nhằm tương thích với các ứng dụng X11 cũ, Wine hoặc Electron.
   - `gaming`: Vô hiệu hóa biến đổi phím `W` đơn lẻ ở đầu từ thành `ư`, giữ nguyên cụm phím di chuyển `WASD`.
3. **Quy tắc chính tả và tốc ký**:
   - Hỗ trợ cả hai quy tắc đặt dấu thanh: truyền thống (`hòa`, `khỏe`, `thủy`) và hiện đại (`hoà`, `khoẻ`, `thuỷ`).
   - Tự động kiểm tra cấu trúc âm tiết và quy tắc thanh điệu âm cuối tắc (`c`, `ch`, `p`, `t`) để khôi phục từ tiếng Anh khi nhấn khoảng trắng.
   - Hỗ trợ gõ tắt phụ âm đầu (`f` -> `ph`, `j` -> `gi`, `w` -> `qu`) và phụ âm cuối (`g` -> `ng`, `h` -> `nh`, `k` -> `ch`).
   - Hỗ trợ tự động viết hoa đầu câu và nhấn đúp phím Cách để chèn dấu chấm câu.

## Yêu cầu hệ thống

Để biên dịch plugin Fcitx5 từ mã nguồn, cần cài đặt trình biên dịch Rust (`stable`), trình biên dịch C++17, `pkg-config` và các tệp tiêu đề phát triển của Fcitx5:

- **Debian / Ubuntu**:
  ```bash
  sudo apt-get install pkg-config build-essential libfcitx5core-dev libfcitx5utils-dev fcitx5
  ```
- **Arch Linux**:
  ```bash
  sudo pacman -S base-devel pkgconf fcitx5
  ```
- **Fedora**:
  ```bash
  sudo dnf install gcc-c++ pkgconf-pkg-config fcitx5-devel fcitx5
  ```

*(Tùy chọn)* Nếu muốn biên dịch `tregai-gui` với tính năng `native-webview` (sử dụng trực tiếp `wry` và `tao` trên GTK3/WebKit2GTK), cần cài đặt thêm `libwebkit2gtk-4.1-dev` và `libgtk-3-dev`. Ở chế độ mặc định, `tregai-gui` không yêu cầu thư viện GTK khi biên dịch.

## Sử dụng và Cài đặt qua Giao diện Đồ họa

Khởi chạy trình quản lý cấu hình:

```bash
cargo run --release --bin tregai-gui
```

Trong cửa sổ giao diện:
1. Chọn **Cài đặt Cấp Người Dùng (~/.local)** để tự động biên dịch `libfcitx5_tregai.so`, sao chép cấu hình addon vào `~/.local`, tạo biểu tượng ứng dụng và khởi động lại Fcitx5.
2. Hoặc chọn **Cài đặt Toàn Hệ Thống (/usr)** để cài đặt thông qua `pkexec`.
3. Mở `fcitx5-configtool` và thêm **Tiếng Việt (Tre Gai)** vào danh sách bộ gõ đang hoạt động.

## Kiểm thử và Phân tích tĩnh (CI)

Dự án đi kèm cấu hình GitHub Actions tại `.github/workflows/ci.yml`. Bạn có thể chạy toàn bộ các bước kiểm tra tương đương trên máy cục bộ:

```bash
# 1. Kiểm tra định dạng mã nguồn
cargo fmt --all -- --check

# 2. Phân tích tĩnh với Clippy (coi mọi cảnh báo là lỗi)
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features fcitx5-plugin -- -D warnings

# 3. Chạy bộ kiểm thử tự động (Unit Tests & FFI Tests)
cargo test --all-targets --features fcitx5-plugin
```

## Phím tắt mặc định

- `Ctrl + Shift + S`: Bật hoặc tắt nhanh chế độ xử lý tiếng Việt ngay trong khi gõ.

## Giấy phép

Phân phối dưới giấy phép **GNU General Public License v3.0 (GPL-3.0-or-later)**. Xem tệp `LICENSE` để biết toàn văn giấy phép.
