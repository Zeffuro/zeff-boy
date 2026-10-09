#[path = "build_support/source_stamp.rs"]
mod source_stamp;

fn main() {
    use sha2::{Digest, Sha256};

    for (cargo, embedded) in [
        ("TARGET", "ZEFF_NETPLAY_BUILD_TARGET"),
        ("PROFILE", "ZEFF_NETPLAY_BUILD_PROFILE"),
        ("OPT_LEVEL", "ZEFF_NETPLAY_BUILD_OPT_LEVEL"),
        ("DEBUG", "ZEFF_NETPLAY_BUILD_DEBUG"),
    ] {
        println!(
            "cargo:rustc-env={embedded}={}",
            std::env::var(cargo).unwrap()
        );
    }
    println!(
        "cargo:rustc-env=ZEFF_NETPLAY_BUILD_FEATURES={}",
        std::env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default()
    );
    let compiler = std::process::Command::new(std::env::var_os("RUSTC").unwrap())
        .arg("-vV")
        .output()
        .expect("read netplay compiler identity");
    assert!(compiler.status.success(), "read netplay compiler identity");
    let fingerprint: String = Sha256::digest(&compiler.stdout)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    println!("cargo:rustc-env=ZEFF_NETPLAY_BUILD_COMPILER={fingerprint}");
    println!("cargo:rerun-if-env-changed=ZEFF_NETPLAY_PIPELINE_OUTPUT_V1");
    let root = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source = source_stamp::snapshot(&root).expect("capture netplay build source");
    for input in source.inputs {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    println!("cargo:rustc-env=ZEFF_NETPLAY_FULL_SOURCE={}", source.full);
    println!(
        "cargo:rustc-env=ZEFF_NETPLAY_QUALIFICATION_SOURCE={}",
        source.qualification
    );
    println!(
        "cargo:rustc-env=ZEFF_NETPLAY_SOURCE_FILES={}",
        source.file_count
    );
    println!("cargo:rerun-if-changed=assets/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winres::WindowsResource::new();
        resources.set_icon("assets/icon.ico");
        resources
            .compile()
            .expect("failed to embed the Windows icon");
    }
}
