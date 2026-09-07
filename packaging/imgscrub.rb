# znznzna/homebrew-tap の Formula/imgscrub.rb に置く。
#
# sha256 は Release の *.tar.gz.sha256 の値。
# リリースのたびに version と 4 つの sha256 を更新する。
class Imgscrub < Formula
  desc "Strip C2PA/AI-provenance and tracking metadata from JPEGs without touching pixels"
  homepage "https://github.com/znznzna/imgscrub"
  version "0.1.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "62b6fb247065d077b2b7ed667a8e24227086fb825da0d1f248af2d5500b95341"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "e1a2d78c91f5dbb5e287f3ec973204c6d8e7657ba27c8d26836c23e553021335"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "cccbca3f58eeb92c67aae2c064d91924bca64891061f81b0a096e77f945c6339"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "d00ae22d11ea8b809e56b817a8f3daa605f2b2f2fc3ae046b3061ed786667d35"
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
