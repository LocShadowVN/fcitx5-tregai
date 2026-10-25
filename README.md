# fcitx5-tregai

**fcitx5-tregai** (Bộ gõ Tre Gai) là bộ gõ tiếng Việt hiệu năng cao dành cho **Fcitx5** trên Linux, được xây dựng độc lập bằng **Rust** kết hợp giao diện **C++20** chuẩn `fcitx::InputMethodEngineV2`.

Dự án hoạt động theo kiến trúc thư viện chia sẻ đơn khối (`libfcitx5_tregai.so`) nạp trực tiếp vào không gian tiến trình của Fcitx5, không sử dụng tiến trình chạy ngầm (`daemon`), không yêu cầu quyền truy cập thiết bị nhân `/dev/uinput` và đi kèm công cụ quản lý cấu hình đồ họa `tregai-gui`.

---

## Tính năng kỹ thuật

| Nhóm tính năng | Chi tiết hỗ trợ |
| :--- | :--- |
| **Phương pháp nhập liệu** | `Telex`, `Simple Telex`, `VNI` (hỗ trợ cả dải phím số Numpad `0-9`), `VIQR` |
| **Bảng mã ký tự** | Unicode dựng sẵn (`NFC`) và Unicode tổ hợp (`NFD`) |
| **Quy tắc dấu thanh** | Truyền thống (`hòa`, `khỏe`, `thủy`) và Hiện đại (`hoà`, `khoẻ`, `thuỷ`), hỗ trợ bỏ dấu tự do cuối từ |
| **Kiểm tra âm vị học** | Phân tích cấu trúc âm tiết (`qu`, `gi`, `ươ`, `uơ`, `oă`, `oao`, `oeo`), luật phối âm `c/k`, `g/gh`, `ng/ngh` và thanh điệu âm cuối tắc (`c`, `ch`, `p`, `t`) |
| **Khôi phục tiếng Anh** | Tự động hoàn tác từ tiếng Anh không hợp lệ âm vị tiếng Việt; nhận diện sớm tiền tố `w-` (`with`, `what`, `window`, `web`, `work`, `world`) ngay từ phím thứ hai |
| **Tốc ký & Soạn thảo** | Gõ tắt phụ âm đầu (`f -> ph`, `j -> gi`, `w -> qu`), phụ âm cuối (`g -> ng`, `h -> nh`, `k -> ch`), tự động viết hoa đầu câu, nhấn đúp phím Cách thành dấu chấm (`. `) |
| **Bảng gõ tắt (Macros)** | Mở rộng từ viết tắt bảo toàn chữ hoa/thường, tương thích định dạng tệp gõ tắt của Unikey |

### Chế độ điều phối xuất chữ (`smooth_mode`)

- **`ultra_smooth`**: Xuất chữ qua `ClientPreedit` với độ trễ 0ms, tối ưu cho môi trường Wayland, GTK4 và Qt6.
- **`smooth`**: Xuất chữ trực tiếp không gạch chân thông qua `SurroundingText` (có kiểm chứng vị trí con trỏ hợp lệ) và tự động chuyển về `Preedit` không gạch chân nếu ứng dụng đích không hỗ trợ `SurroundingText`.
- **`slow_compat`**: Chế độ xuất chữ trực tiếp tương thích với các ứng dụng X11 cũ và Wine.
- **`gaming`**: Vô hiệu hóa biến đổi phím `W` đơn lẻ ở đầu từ thành `ư`, bảo toàn cụm phím di chuyển `WASD` và tự động bỏ qua danh sách tiến trình trò chơi trong `excluded_apps`.

---

## Cài đặt

### 1. Cài đặt từ gói phát hành sẵn (Khuyên dùng)

Tải gói cài đặt tương ứng tại trang [Releases](../../releases):

**Debian / Ubuntu / Linux Mint / Pop!_OS (`.deb`):**
```bash
sudo apt install ./fcitx5-tregai_*_amd64.deb
fcitx5 -r -d
```

**Arch Linux / Fedora / Bản phân phối khác (`.tar.gz`):**
```bash
tar -xzf fcitx5-tregai-*-x86_64-linux.tar.gz
cd fcitx5-tregai-*-x86_64-linux
./install.sh
fcitx5 -r -d
```

### 2. Biên dịch và cài đặt từ mã nguồn

#### Yêu cầu hệ thống
Cần cài đặt trình biên dịch Rust (`stable`), trình biên dịch hỗ trợ **C++20** (`gcc` hoặc `clang`), `pkg-config` và các gói tiêu đề phát triển của Fcitx5:

- **Debian / Ubuntu**:
  ```bash
  sudo apt-get update
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

*(Tùy chọn)* Nếu biên dịch `tregai-gui` kèm cờ tính năng `native-webview` (sử dụng trực tiếp `wry` và `tao` trên WebKit2GTK), cần cài đặt thêm `libwebkit2gtk-4.1-dev` và `libgtk-3-dev`. Ở chế độ mặc định, `tregai-gui` không yêu cầu thư viện GTK khi biên dịch.

#### Biên dịch và cài đặt qua `tregai-gui`
```bash
cargo run --release --bin tregai-gui
```
Trong cửa sổ quản lý cấu hình:
1. Chọn **Cài đặt Cấp Người Dùng (`~/.local`)** để biên dịch `libfcitx5_tregai.so`, sao chép cấu hình addon vào `~/.local` và khởi động lại Fcitx5 mà không cần quyền `root`.
2. Hoặc chọn **Cài đặt Toàn Hệ Thống (`/usr`)** để cài đặt vào hệ thống thông qua `pkexec`.
3. Mở `fcitx5-configtool` và thêm **Tiếng Việt (Tre Gai)** vào danh sách bộ gõ đang hoạt động.

---

## Cấu hình và Phím tắt

### Phím tắt mặc định
- **`Ctrl + Shift + S`**: Bật hoặc tắt nhanh chế độ xử lý tiếng Việt ngay trong khi gõ.

### Tệp cấu hình
Cấu hình được lưu trữ theo chuẩn INI tại `$XDG_CONFIG_HOME/fcitx5-tregai/config.ini` (mặc định là `~/.config/fcitx5-tregai/config.ini`) và hỗ trợ nạp nóng qua sự kiện `reloadConfig` của Fcitx5:

```ini
[general]
enabled = true
method = telex
charset = unicode
smooth_mode = ultra_smooth
commit_delay_ms = 15
underline_preedit = true
modern_tone = false
spell_check = true
auto_restore_english = true
free_tone = true
w_to_u = true
bracket_shortcuts = true
quick_start_consonant = false
quick_end_consonant = false
auto_capitalize = false
double_space_period = false
allow_foreign_consonants = false
enable_macros = true
macro_in_english_mode = false
excluded_apps = steam, cs2, dota2, lutris

[macros]
vn = Việt Nam
hn = Hà Nội
tphcm = Thành phố Hồ Chí Minh
```

---

## Kiến trúc mã nguồn

```text
fcitx5-tregai/
├── build.rs                 # Kịch bản liên kết pkg-config (Fcitx5Core, Fcitx5Utils) và biên dịch C++20
├── csrc/
│   └── fcitx5_bridge.cpp    # Triển khai fcitx::InputMethodEngineV2 và điều phối SurroundingText / Preedit
├── data/
│   ├── tregai-addon.conf    # Định nghĩa thông tin Addon cho Fcitx5
│   ├── tregai.conf          # Định nghĩa mục InputMethod cho Fcitx5
│   └── tregai-gui.desktop   # Biểu tượng ứng dụng cấu hình trên Desktop Environment
└── src/
    ├── lib.rs               # Điểm xuất thư viện và bộ kiểm thử tích hợp (Unit & FFI Tests)
    ├── unicode.rs           # Bảng ánh xạ ký tự tiếng Việt NFC/NFD với độ phức tạp O(1)
    ├── syllable.rs          # Bộ phân tích cấu trúc âm tiết, định vị dấu thanh và kiểm tra chính tả
    ├── engine.rs            # Máy trạng thái xử lý phím, hoàn tác phím lặp và khôi phục tiếng Anh
    ├── config.rs            # Trình phân tích cấu hình INI, lọc tiến trình loại trừ và xử lý Macro
    ├── ffi.rs               # Giao diện C-ABI kết nối giữa lớp C++ Fcitx5 và nhân Rust
    └── bin/
        └── tregai-gui.rs    # Ứng dụng quản lý cấu hình, cài đặt và kiểm thử trực tiếp (Sandbox)
```

---

## Kiểm thử và Phân tích tĩnh (CI)

Dự án tích hợp quy trình kiểm tra tự động qua GitHub Actions (`.github/workflows/ci.yml`). Để thực thi toàn bộ quy trình kiểm tra trên máy cục bộ:

```bash
# 1. Kiểm tra định dạng mã nguồn
cargo fmt --all -- --check

# 2. Phân tích tĩnh với Clippy
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --features fcitx5-plugin -- -D warnings

# 3. Chạy bộ kiểm thử tự động (Unit Tests & FFI Tests)
cargo test --all-targets --features fcitx5-plugin --verbose

# 4. Biên dịch bản phát hành (Release)
cargo build --release --features fcitx5-plugin
```

---

## Giấy phép

Phân phối dưới giấy phép **GNU General Public License v3.0 (GPL-3.0-or-later)**. Xem tệp [`LICENSE`](LICENSE) để biết toàn văn giấy phép.
