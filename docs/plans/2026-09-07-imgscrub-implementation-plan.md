# imgscrub v0.1 実装計画

- 作成日: 2026-09-07
- 設計書: [2026-09-07-imgscrub-design.md](2026-09-07-imgscrub-design.md)（承認済み）
- 実装先: `/Volumes/Workspace/dev/imgscrub/`（新規リポジトリ）
- 公開先: `github.com/znznzna/imgscrub`（public）

## 進め方の原則

- **縦切りで最初に動くものを作る。** Phase 2 の終わりで `--c2pa-only` が実ファイルに対して動き、
  設計書 §12 の実測値を再現できる状態にする。以降は横に広げるだけにする
- **不変条件テストを機能より先に書く。** SOS バイト列同一性・冪等性・入力無変更は
  後付けでは担保できない。Phase 1 でフィクスチャとテスト基盤を先に立てる
- **各 Phase の終わりに `.claude/verify.sh fast` が通ることを完了条件にする**

## Phase 0: リポジトリ初期化

- [ ] `cargo init --bin /Volumes/Workspace/dev/imgscrub` / `git init`
- [ ] 設計書と本計画を `docs/plans/` にコピー
- [ ] `.claude/verify.sh` を作成（fast/full 契約）

  | 契約 | 内容 |
  |---|---|
  | `fast` | `cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo build` |
  | `full` | fast + `cargo test` |

- [ ] `LICENSE`（MIT）/ `.gitignore`（`/target`）
- [ ] `Cargo.toml`: `clap` (derive), `quick-xml`, `ciborium`。`[profile.release] lto = true, strip = true`
- [ ] `README.md` の見出しだけ置く（中身は Phase 8）
- [ ] `.github/workflows/ci.yml` — `ubuntu-latest` / `macos-latest`、**GitHub-hosted**
      （public リポなので self-hosted は使わない）

**完了条件**: `cargo build` が通り、CI が green

## Phase 1: セグメント走査とテスト基盤

ここが全体の土台。**フィクスチャ生成を最初にやる。**

### 1.1 フィクスチャ生成ツール

`tools/mkfixture.py`（Python。リポジトリに含めるが CI では実行しない）

実写真から**画素だけ 16x16 に縮小し、APP11 / APP1(XMP) / APP1(Exif) / APP13 は実物のバイト列を移植**する。

> C2PA の hash アサーションは縮小後の画素と一致しなくなるが、テスト対象は
> セグメント外科手術であって C2PA 検証ではないため問題にならない。この点を
> フィクスチャの README に明記しておく（後で混乱の種になるため）。

| フィクスチャ | 作り方 |
|---|---|
| `lrc_firefly.jpg` | `6x7-18_PSMS_edited.jpg` から生成。APP11 + `crs:fill_method=firefly` |
| `lrc_clean.jpg` | Firefly 未使用の LrC 書き出しから生成（**要: 該当ファイルの用意**） |
| `camera_mpf.jpg` | APP2 `MPF\0` を**合成して**付与する（他人の写真を含めないため） |
| `no_exif.jpg` | `cjpeg` で生成した素の JPEG |
| `truncated.jpg` | `lrc_firefly.jpg` の SOS を途中で切り詰め |

### 1.2 `jpeg/segment.rs`

- [ ] マーカー走査イテレータ。`SOI` 検証 → 各セグメント `(marker, offset, len, payload)` を返す
- [ ] **`SOS` 到達で走査を終了**（設計書 §4.4。スキャン内の `0xFF00` と RST を誤検出しないため）
- [ ] スタンドアロンマーカー（`0x01`, `0xD0`–`0xD7`）は長さフィールドを持たない扱い
- [ ] 異常系: SOI なし / 長さフィールドがファイル末尾を超える / SOS が来ないまま EOF

### 1.3 テスト基盤

- [ ] `tests/invariants.rs` — 全フィクスチャに対する共通不変条件（設計書 §8）
- [ ] ヘルパ: `sos_bytes(path)` / `segment_digests(path)` / `decode_pixel_hash(path)`

**完了条件**: `lrc_firefly.jpg` のセグメント一覧が設計書 §1.1 の構成と一致することをテストで確認

## Phase 2: 除去と検証 — 最初の縦切り

- [ ] `jpeg/filter.rs` — 設計書 §4.1 の保持/削除表を実装
  - [ ] APP11 の C2PA 判定（payload が `JP` 始まり）
  - [ ] **APP2 の `MPF\0` を削除**し報告する（§4.3。オフセット破綻の回避）
  - [ ] **APP14 (Adobe) を保持**（§4.1。落とすと色解釈が変わる）
  - [ ] 未知の APPn は削除（allowlist 思想）
- [ ] 書き出し: 一時ファイル → `verify` → atomic rename
- [ ] `jpeg/verify.rs`
  - [ ] 出力の再パースが成功すること
  - [ ] **SOS バイト列が入力と完全一致**すること
  - [ ] 失敗時は一時ファイルを破棄し入力を無変更のまま残す
- [ ] `--c2pa-only` の経路をここで完成させる

**完了条件（縦切りの検証）**: 実ファイルに対して以下を再現する

```
入力  6x7-18_PSMS_edited.jpg   17,894,419 B
出力  --c2pa-only              -14,478 B（APP11 のみ削除）
EXIF / ICC / IPTC / XMP  : バイト単位で一致
デコード後画素 SHA256     : 7788fcf8c4c542f3e9c6f033852ef9a0
```

## Phase 3: XMP プロパティ除去

- [ ] `xmp.rs` — `quick-xml` で読み、**プロパティ単位**で除去して再直列化
      （正規表現は使わない。名前空間宣言と CDATA で壊れるため）
- [ ] 除去リスト（設計書 §4.1）: `crs:RemoveAreas`, `crs:fill_method`,
      `xmpMM:PreservedFileName`, `xmpMM:DocumentID` / `InstanceID` / `OriginalDocumentID`,
      `xmpMM:History*`, `xmpMM:DerivedFrom*`, `dcterms:provenance`
- [ ] 保持: `dc:*`, `photoshop:*`, `Iptc4xmpCore:*`, `Iptc4xmpExt:*`, `aux:*`, `exifEX:*`
- [ ] 除去後に XMP が空になった場合は APP1/XMP セグメント自体を落とす
- [ ] 拡張 XMP（`xmpNote:HasExtendedXMP` + 追加 APP1）の検出 → v0.1 では**警告して XMP を無編集で保持**

**完了条件**: `lrc_firefly.jpg` から firefly 痕跡が消え、`XMP-dc:Rights` が残ることをテストで確認。
`-18,660 B` を実ファイルで再現する

## Phase 4: inspect

- [ ] `jumbf.rs` — JUMBF ボックス階層を歩き、`c2pa.actions.v2` を `ciborium` でデコード
- [ ] 既知フィールド優先で抽出: `digitalSourceType`, `softwareAgent.name/version`,
      `com.adobe.acr.value`, `com.adobe.firefly.version`, `com.adobe.appEnforced`
- [ ] 署名者と issuer を出す（`Adobe Compliance Signer` / `c2pa-ephemeral-ca.local`）
- [ ] `digitalSourceType` を検出したら **X の AI 判定原因として明示**
- [ ] `crs:fill_method=firefly` を検出したら**予防ヒント**を出す
      （「Remove ツールの生成AI をオフに」— 設計書 §5 の出力例どおり）
- [ ] `inspect` はファイルを変更しない（前後ハッシュ一致をテスト）

**完了条件**: 設計書 §5 の `inspect` 出力例を実ファイルで再現

## Phase 5: CLI 表面

- [ ] `clap` で設計書 §5 のインターフェースを実装
- [ ] 既定は**非破壊**（`<name>_clean.jpg` を隣に作成）。`--in-place` は明示的オプトイン
- [ ] `-r` 再帰 / `-n` dry-run / `-q` / `--json` / `-o` / `--keep <LIST>`
- [ ] **`--strip-exif-private` は実装せず**、GPS・`BodySerialNumber`・`LensSerialNumber`・
      `CameraOwnerName`・MakerNote を**検出したら警告のみ**（v0.2 送り。設計書 §4.2）
- [ ] 終了コード（設計書 §7）: スキップ・already clean は **0**、検証失敗・壊れた JPEG は **2**
- [ ] 冪等性: 2 回目は `already clean` で exit 0

**完了条件**: 設計書 §7 の表の全ケースをテストで網羅

## Phase 6: Lightroom Classic 統合

- [ ] `imgscrub install-lightroom-action`
      → `~/Library/Application Support/Adobe/Lightroom/Export Actions/` に
        `imgscrub.sh`（`exec imgscrub --in-place --quiet "$@"`）を設置
- [ ] 既存ファイルがある場合は上書き確認
- [ ] `uninstall-lightroom-action` も用意
- [ ] **手動での実機確認**: LrC の書き出しダイアログ「後処理」に現れ、
      書き出し後に APP11 が消えていること

**完了条件**: 実際に LrC から書き出して C2PA が付いていないことを `inspect` で確認

## Phase 7: 配布

- [ ] `.github/workflows/release.yml` — タグ push で 4 ターゲットをビルドして Releases に添付
      （macOS aarch64/x86_64、Linux x86_64/aarch64）
- [ ] `znznzna/homebrew-tap` を新規作成し `Formula/imgscrub.rb` を置く
- [ ] crates.io へ publish（名前は空き確認済み）
- [ ] `v0.1.0` タグ

## Phase 8: README

設計書 §10 の構成で書く。順序が重要。

1. これは何か
2. **なぜ作ったか** — 設計書 §1 の実測（強制付与 + 一時 CA の非対称性）
3. **そもそも付けない方法**（Remove の生成AI をオフ / 修復ブラシ / コピースタンプ）
4. 使い方 / Lightroom 統合
5. 何を消して何を残すか（§4.1 の表）
6. 何を保証するか（画素無劣化・EXIF バイト無傷・冪等・入力を壊さない）
7. C2PA についての注記（除去は provenance の主張を失うことでもある）

- [ ] 日本語版と英語版（`README.md` を英語、`README.ja.md` を日本語。公開ツールなので）

## リスクと対処

| リスク | 対処 |
|---|---|
| 拡張 XMP を持つファイルで XMP 編集が壊れる | Phase 3 で検出したら XMP を無編集で保持し警告 |
| MPF 以外にも絶対オフセットを持つ APPn がある | 未知の APPn は既定で削除する設計なので影響しない |
| `lrc_clean.jpg` フィクスチャ用のファイルがない | Firefly を使わず 1 枚書き出して用意する（要作業） |
| `quick-xml` の再直列化で属性順序が変わる | 不変条件テストは XMP のバイト一致を要求しない（他セグメントのみ） |
| LrC のバージョン差で C2PA 構造が変わる | `inspect` は既知フィールドを見つけられなくても落ちない実装にする |

## 未確定（実装中に決める）

1. `--json` スキーマの形（`inspect` と実行サマリで共通にするか分けるか）
2. `--keep` の語彙（`gps` は v0.1 では効果なしなので v0.1 では受け付けないか、警告するか）

## 完了状況（2026-09-07）

| Phase | 状態 | 記録 |
|---|---|---|
| 0 リポジトリ初期化 | 完了 | `.claude/verify.sh`（fast/full）、CI は GitHub-hosted |
| 1 セグメント走査 + フィクスチャ | 完了 | 実物の APPn を移植した 5 種、テスト 12 件 |
| 2 除去と検証（縦切り） | 完了 | `--c2pa-only` で -14,478 B、他セグメントはバイト単位一致 |
| 3 XMP プロパティ除去 | 完了 | 既定で -24,147 B、firefly 痕跡ゼロ |
| 4 inspect | 完了 | appEnforced と署名者/発行者の分離まで読める |
| 5 CLI | 完了 | `--keep` / `-r` / `--json` / 終了コード |
| 6 Lightroom 統合 | 完了 | Export Actions に登録・実機確認 |
| 7 配布 | 完了（crates.io を除く） | 下記 |
| 8 README | 完了 | 英日 |

### 配布の結果

- リポジトリ: <https://github.com/znznzna/imgscrub>（public）
- CI: ubuntu-latest / macos-latest で green
- Release `v0.1.0`: 4 ターゲットの tar.gz + sha256
- Homebrew tap: <https://github.com/znznzna/homebrew-tap>
  `brew install znznzna/tap/imgscrub` で実機インストール確認済み
- **crates.io: 未公開。** `cargo publish --dry-run` は通るが API トークンが無い。
  `cargo login` の後に `cargo publish` で完了する

### テスト

42 件（segments 12 / invariants 9 / xmp 10 / inspect 11）。`verify.sh full` 通過。

### 設計から変えた点

1. **MPF の削除条件を厳密化** — 「先行セグメントが 1 つでも削除されたら削除」に変更。
   何も削除しないなら MPF は残せる
2. **xpacket パディング（約 4KB）を落とす** — 他ツールの in-place 編集用の余白で、
   常に全体を書き直す imgscrub には不要。設計時の PoC より削除量が増えた
3. **マニフェストストアの全マニフェストを走査** — C2PA はストアに複数のマニフェストを
   持ちうる。先頭だけ見ると取り込んだ素材由来の AI 宣言を取りこぼす
4. **`Segment::len()` → `size()`** — セグメントは最小 2 バイトで空になり得ないため、
   `len`/`is_empty` の対を持たせる意味がない

### 実装中に見つけたバグ

- **quick-xml の `check_end_names` は EOF 時点の未閉タグを検出しない。**
  壊れた XMP が黙って「別の妥当な XML」に書き換わる状態だった。EOF で深さ 0 を
  要求する検査を追加し、壊れている場合は無編集保持 + 警告に落とす

### 残件

1. crates.io への publish（要 `cargo login`）
2. `--strip-exif-private`（GPS・シリアル・MakerNote の除去。IFD 再構築）→ v0.2
3. PNG / TIFF 対応 → v0.2 以降
