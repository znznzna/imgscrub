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
      sha256 "d01f59e646eefee432b3e9c027ef1faa7cdab3b5d0fa46fc4bf3f116161586d9"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "d14463e623bf259e1a517d1475f01a34c26d44d4581cb329b263cbf3863a5966"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "839e9c764b72443e867ffecf47e223242e34a2288cf14fbdf2d7b95991ade76b"
    end
    on_intel do
      url "https://github.com/znznzna/imgscrub/releases/download/v#{version}/imgscrub-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "6d0efca0f98023c9cf9fac4a8622a9e0e2a42c5a288e436583a54843ab915db0"
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
