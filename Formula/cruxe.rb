class Cruxe < Formula
  desc "Code search and navigation engine for AI coding assistants"
  homepage "https://github.com/signalridge/cruxe"
  license "MIT"
  # Placeholder seed formula. `.github/workflows/homebrew-update.yml`
  # rewrites URLs + SHA256 values from published release assets.
  version "0.1.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/signalridge/cruxe/releases/download/v0.1.0/cruxe-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    else
      url "https://github.com/signalridge/cruxe/releases/download/v0.1.0/cruxe-v0.1.0-x86_64-apple-darwin.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/signalridge/cruxe/releases/download/v0.1.0/cruxe-v0.1.0-aarch64-unknown-linux-musl.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    else
      url "https://github.com/signalridge/cruxe/releases/download/v0.1.0/cruxe-v0.1.0-x86_64-unknown-linux-musl.tar.gz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "cruxe"
  end

  test do
    output = shell_output("#{bin}/cruxe --version")
    assert_match(version.to_s, output)
  end
end
