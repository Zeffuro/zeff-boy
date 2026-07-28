fn main() -> anyhow::Result<()> {
    zeff_romtest::run(std::env::args_os().skip(1))
}
