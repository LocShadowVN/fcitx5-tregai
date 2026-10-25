// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    println!("cargo:rerun-if-changed=csrc/fcitx5_bridge.cpp");

    #[cfg(feature = "fcitx5-plugin")]
    {
        let core = pkg_config::Config::new()
            .atleast_version("5.0")
            .probe("Fcitx5Core")
            .expect("Fcitx5Core not found via pkg-config. Install libfcitx5core-dev / fcitx5.");
        let utils = pkg_config::Config::new()
            .atleast_version("5.0")
            .probe("Fcitx5Utils")
            .expect("Fcitx5Utils not found via pkg-config. Install libfcitx5utils-dev / fcitx5.");

        let mut build = cc::Build::new();
        build
            .cpp(true)
            .std("c++20")
            .file("csrc/fcitx5_bridge.cpp")
            .flag_if_supported("-Wall")
            .flag_if_supported("-Wextra")
            .flag_if_supported("-fPIC");

        for path in core.include_paths.iter().chain(utils.include_paths.iter()) {
            build.include(path);
        }

        build.compile("fcitx5_tregai_bridge");

        for lib in core.libs.iter().chain(utils.libs.iter()) {
            println!("cargo:rustc-link-lib={}", lib);
        }
    }
}
