fn main() {
    println!("cargo:rerun-if-changed=src/libretro_harness_log.c");
    if std::env::var("TARGET").is_ok_and(|target| target.starts_with("wasm")) {
        return;
    }
    cc::Build::new()
        .file("src/libretro_harness_log.c")
        .compile("zeff_libretro_harness_log");
}
