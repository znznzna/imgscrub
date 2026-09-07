#!/usr/bin/env python3
"""テスト用フィクスチャ生成ツール。

実写真から画素だけを 16x16 に縮小し、APPn セグメント（C2PA / EXIF / XMP / IPTC /
ICC / Adobe）は実物のバイト列をそのまま移植する。

注意: C2PA の hash アサーションは縮小後の画素と一致しなくなる。imgscrub のテスト対象は
セグメントの外科手術であって C2PA の署名検証ではないため、これは問題にならない。
将来 C2PA 検証機能を足す場合はフィクスチャを作り直す必要がある。

CI では実行しない（Pillow に依存するため）。生成物をリポジトリにコミットして使う。
"""
import io
import sys
from pathlib import Path

from PIL import Image

SOS = 0xDA
EOI = 0xD9
STANDALONE = {0x01} | set(range(0xD0, 0xD8))


def segments(d: bytes):
    """(marker, start, end, payload) を返す。SOS 到達で打ち切る。"""
    if d[:2] != b"\xff\xd8":
        raise ValueError("SOI がない")
    i = 2
    while i < len(d) - 1:
        if d[i] != 0xFF:
            i += 1
            continue
        m = d[i + 1]
        if m == 0xFF:
            i += 1
            continue
        if m in STANDALONE:
            yield (m, i, i + 2, None)
            i += 2
            continue
        if m == SOS:
            yield (m, i, len(d), None)
            return
        if m == EOI:
            yield (m, i, i + 2, None)
            i += 2
            continue
        ln = int.from_bytes(d[i + 2 : i + 4], "big")
        yield (m, i, i + 2 + ln, d[i + 4 : i + 2 + ln])
        i += 2 + ln


def is_app(m):
    return 0xE0 <= m <= 0xEF


def tiny_body(src_path: Path, size=16) -> bytes:
    """縮小した JPEG から APPn を除いた本体（DQT 以降）を取り出す。"""
    Image.MAX_IMAGE_PIXELS = None
    im = Image.open(src_path).convert("RGB").resize((size, size), Image.LANCZOS)
    buf = io.BytesIO()
    im.save(buf, format="JPEG", quality=90, subsampling=0)
    d = buf.getvalue()
    return b"".join(d[s:e] for m, s, e, _ in segments(d) if not is_app(m))


def metadata_segments(src_path: Path) -> bytes:
    """元ファイルの APPn セグメントをバイト列のまま取り出す。"""
    d = src_path.read_bytes()
    return b"".join(d[s:e] for m, s, e, _ in segments(d) if is_app(m))


def transplant(src: Path, out: Path, size=16):
    data = b"\xff\xd8" + metadata_segments(src) + tiny_body(src, size)
    out.write_bytes(data)
    return data


def synth_mpf(base: Path, out: Path):
    """APP2 'MPF\\0' セグメントを付与する。

    中身は構造的なスタブ。imgscrub は MPF を削除する（オフセットが破綻するため）ので、
    テストに必要なのはシグネチャの検出だけで、IFD の妥当性は要求されない。
    """
    d = base.read_bytes()
    # MPF: 'MPF\0' + TIFF ヘッダ + 最小の MP Index IFD（エントリ 0 件）
    payload = b"MPF\x00" + b"II\x2a\x00" + (8).to_bytes(4, "little") + (0).to_bytes(2, "little")
    seg = b"\xff\xe2" + (len(payload) + 2).to_bytes(2, "big") + payload
    # 先頭の APPn 群の直後に挿入する
    segs = list(segments(d))
    ins = next(s for m, s, e, _ in segs if not is_app(m))
    out.write_bytes(d[:ins] + seg + d[ins:])


def main():
    root = Path(__file__).resolve().parent.parent
    fx = root / "tests" / "fixtures"
    fx.mkdir(parents=True, exist_ok=True)

    export = Path("/Volumes/Workspace/Lightroom Local/Local Export")
    sources = {
        "lrc_firefly.jpg": export / "6x7-18_PSMS_edited.jpg",
        "lrc_clean.jpg": export / "6x7-10_PSMS_edited.jpg",
    }

    for name, src in sources.items():
        if not src.exists():
            print(f"  SKIP {name}: 元ファイルがない ({src})", file=sys.stderr)
            continue
        data = transplant(src, fx / name)
        print(f"  {name}: {len(data):,} B  <- {src.name}")

    # MPF 付き（カメラ出し JPEG の模擬）
    if (fx / "lrc_clean.jpg").exists():
        synth_mpf(fx / "lrc_clean.jpg", fx / "camera_mpf.jpg")
        print(f"  camera_mpf.jpg: {(fx / 'camera_mpf.jpg').stat().st_size:,} B  (APP2/MPF 合成)")

    # メタデータほぼ無し
    Image.new("RGB", (16, 16), (128, 90, 60)).save(fx / "no_exif.jpg", quality=90)
    print(f"  no_exif.jpg: {(fx / 'no_exif.jpg').stat().st_size:,} B")

    # 異常系: SOS を途中で切り詰め
    src = (fx / "lrc_firefly.jpg").read_bytes()
    sos = next(s for m, s, e, _ in segments(src) if m == SOS)
    (fx / "truncated.jpg").write_bytes(src[: sos + 40])
    print(f"  truncated.jpg: {(fx / 'truncated.jpg').stat().st_size:,} B  (SOS 切り詰め)")


if __name__ == "__main__":
    main()
