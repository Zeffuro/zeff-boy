extern crate cc;

use std::env;

fn add_def(v: &mut Vec<(String, String)>, key: &str, val: &str) {
    v.push((key.to_owned(), val.to_owned()));
}

fn main() {
    const VENDOR_DIR: &str = "vendor";
    const SOURCE: &str = "vendor/xdelta3.c";

    println!("cargo:rerun-if-changed={VENDOR_DIR}");
    let mut defines = Vec::new();
    let pointer_bytes = env::var("CARGO_CFG_TARGET_POINTER_WIDTH")
        .expect("target pointer width")
        .parse::<u32>()
        .expect("numeric target pointer width")
        / 8;
    let unsigned_long_bytes = if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        4
    } else {
        pointer_bytes
    };
    add_def(&mut defines, "SIZEOF_SIZE_T", &pointer_bytes.to_string());
    add_def(&mut defines, "SIZEOF_UNSIGNED_INT", "4");
    add_def(
        &mut defines,
        "SIZEOF_UNSIGNED_LONG",
        &unsigned_long_bytes.to_string(),
    );
    add_def(&mut defines, "SIZEOF_UNSIGNED_LONG_LONG", "8");
    add_def(&mut defines, "SECONDARY_DJW", "1");
    add_def(&mut defines, "SECONDARY_FGK", "1");
    add_def(&mut defines, "EXTERNAL_COMPRESSION", "0");
    add_def(&mut defines, "XD3_USE_LARGEFILE64", "1");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        add_def(&mut defines, "XD3_WIN32", "1");
    }
    add_def(&mut defines, "SHELL_TESTS", "0");

    {
        let mut builder = cc::Build::new();
        builder.include(VENDOR_DIR);
        for (key, val) in &defines {
            builder.define(key, Some(val.as_str()));
        }

        builder.file(SOURCE).warnings(false).compile("xdelta3");
    }
}
