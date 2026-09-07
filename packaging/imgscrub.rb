# znznzna/homebrew-tap の Formula/imgscrub.rb に置く。
#
# sha256 は Release の *.tar.gz.sha256 の値に差し替える。
# リリースのたびに version / url / sha256 を更新する。
class Imgscrub < Formula
  desc "Strip C2PA/AI-provenance and tracking metadata from JPEGs without touching pixels"
  homepage "https://github.com/znznzna/imgscrub"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "REPLACE_ME_AARCH64_DARWIN"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "REPLACE_ME_X86_64_DARWIN"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_ME_AARCH64_LINUX"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "REPLACE_ME_X86_64_LINUX"
    end
  end

  def install
    bin.install "imgscrub"
    doc.install "README.md", "README.ja.md"
  end

  test do
    assert_match "imgscrub", shell_output("#{bin}/imgscrub --version")
  end
end
