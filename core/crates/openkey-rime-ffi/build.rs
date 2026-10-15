use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=LIBRIME_DIR");
    println!("cargo:rerun-if-env-changed=LIBRIME_LIB_DIR");
    println!("cargo:rerun-if-env-changed=LIBRIME_INCLUDE_DIR");

    if !cfg!(feature = "librime") {
        // 未启用 librime 特性：不链接任何原生库。
        return;
    }

    if let Some(dir) = std::env::var_os("LIBRIME_LIB_DIR") {
        println!("cargo:rustc-link-search=native={}", dir.to_string_lossy());
    } else if let Ok(prefix) = std::env::var("LIBRIME_DIR") {
        let prefix = PathBuf::from(prefix);
        println!(
            "cargo:rustc-link-search=native={}",
            prefix.join("lib").display()
        );
        println!(
            "cargo:rustc-link-search=native={}",
            prefix.join("lib64").display()
        );
    }

    // librime 以动态库方式链接；安装包需随附 librime 共享库（见 THIRD_PARTY_NOTICES）。
    println!("cargo:rustc-link-lib=dylib=rime");
}
