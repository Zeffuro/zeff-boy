cask "zeff-boy" do
  version "0.1.0"
  sha256 ""

  url "https://github.com/Zeffuro/zeff-boy/releases/download/v#{version}/zeff-boy-v#{version}-aarch64-apple-darwin.dmg"
  name "zeff-boy"
  desc "A Game Boy, Game Boy Advance, NES, WonderSwan, and Sega 8-bit emulator written in Rust"
  homepage "https://github.com/Zeffuro/zeff-boy"

  depends_on arch: :arm64

  app "Zeff Boy.app"

  livecheck do
    url :url
    strategy :github_latest
  end
end

