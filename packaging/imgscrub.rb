# znznzna/homebrew-tap の Formula/imgscrub.rb に置く。
#
# sha256 は Release の *.tar.gz.sha256 の値。
# リリースのたびに version と 4 つの sha256 を更新する。
class Imgscrub < Formula
  desc "Strip C2PA/AI-provenance and tracking metadata from JPEGs without touching pixels"
  homepage "https://github.com/znznzna/imgscrub"
  version "0.1.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "9170782cc6ae0b85c78712b2390489d13ead1ca72848afe9477673caeea1d201"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "4042ae4278ec4d8405258b55b2719a0f2b191cf1d2aacaf12716606c019650ce"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "a03ab8087255f0ecdb8bffde6acf9cf5169f4fb2c74ea93367468824279472e6"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "42ac3a87f5853956d795e1371056d7c3f35e800e46e57615926a099aa3d95a38"
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
