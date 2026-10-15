use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let crate_dir = PathBuf::from(&manifest_dir);
    let out_dir = crate_dir.join("../../ffi-headers");
    std::fs::create_dir_all(&out_dir).ok();

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_language(cbindgen::Language::C)
        .with_include_guard("OPENKEY_H")
        .with_header(
            "/*\n * OpenKey C ABI（由 core/crates/openkey-ffi 的 cbindgen 自动生成，请勿手改）。\n\
             * 生成命令：cd core && cargo build -p openkey-ffi\n\
             * 五端客户端只应包含本头文件与 core 交互（docs/02 2.1 职责边界铁律）。\n\
             *\n\
             * 字符串约定：\n\
             *  - *_buf/len 形式的输出：调用方提供缓冲区，函数写入 NUL 结尾 UTF-8；\n\
             *    缓冲区不足返回 false。\n\
             *  - 直接返回 const char* 的：指针归对象所有，下次对同一对象调用前有效。\n\
             *  - 错误详情：ok_last_error() 返回，直至下次出错前有效。\n\
             */",
        )
        .generate()
        .expect("cbindgen 生成 openkey.h 失败")
        .write_to_file(out_dir.join("openkey.h"));

    println!(
        "cargo:warning=openkey.h 已生成至 {}",
        out_dir.join("openkey.h").display()
    );
}
