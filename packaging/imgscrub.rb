# znznzna/homebrew-tap の Formula/imgscrub.rb に置く。
#
# sha256 は Release の *.tar.gz.sha256 の値。
# リリースのたびに version と 4 つの sha256 を更新する。
class Imgscrub < Formula
  desc "Strip C2PA/AI-provenance and tracking metadata from JPEGs without touching pixels"
  homepage "https://github.com/znznzna/imgscrub"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "7167671a924b4ad85c2426fc530973fd533cc9e0f76acc38643cfc1bd06c3829"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "6d1c63b87ed4b5c5e837cf9dc64b8daf12288f2e20dd2ea8713e6d98d68a6e8a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "40022c5cc7c5f3af96596d4ab9dbe20c041734905e867f4f299b5376020ad20f"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "870e60c83aeb29c051150c48646df741dcab0844d24bfbf0c11cfb4a117a970b"
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
