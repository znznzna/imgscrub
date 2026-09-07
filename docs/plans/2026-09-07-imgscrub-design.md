# imgscrub 設計書

- 作成日: 2026-09-07
- ステータス: 設計（未承認 / 全設計判断は確定済み）
- フレーミング: F2 書き出し衛生（allowlist 思想）
- 対象フォーマット: JPEG のみ（v0.1）
- 言語: Rust（単一バイナリ配布）

## 1. 背景 — 実測で確定した事実

Lightroom Classic 15.5.1 から「Content Credentials オフ」で書き出した JPEG を実測した結果。

対象: `/Volumes/Workspace/Lightroom Local/Local Export/6x7-18_PSMS_edited.jpg`（17,894,419 bytes）

### 1.1 C2PA マニフェストは強制付与されている

書き出しダイアログの設定に関係なく、APP11 セグメント（14,478 bytes）に完全な C2PA マニフェストが埋まっていた。

```
JPEG
├─ APP11 (14,478 B) ← JUMBF = C2PA マニフェスト
│   └─ urn:c2pa:e7a6bef0-e990-48eb-91a9-220bec674373:adobe
│       ├─ c2pa.assertions
│       │   ├─ c2pa.actions.v2   ← X が読む実体
│       │   ├─ c2pa.ingredient.v3
│       │   └─ c2pa.hash.data
│       ├─ c2pa.claim.v2         (Adobe Lightroom Classic 15.5.1)
│       └─ c2pa.signature
├─ APP1/EXIF   (8,855 B)
├─ APP13/IPTC  (8,010 B)
├─ APP2/ICC    (3,162 B)
├─ APP1/XMP    (15,778 B)  crs:fill_method="firefly"
└─ DQT / DRI / APP14 / SOF0 / DHT / SOS
```

`c2pa.actions.v2` の中身:

| フィールド | 値 |
|---|---|
| `softwareAgent.name` | Adobe Remove Object |
| `com.adobe.acr.value` | Healing changed, **Uses GenAI** |
| `com.adobe.firefly.version` | `clio-erase-2.0#epoch=0-step=4000.clio-erase-c2-2025-03-19T10-37-32` |
| `digitalSourceType` | `http://cv.iptc.org/newscodes/digitalsourcetype/compositeWithTrainedAlgorithmicMedia` |
| `com.adobe.appEnforced` | **true** |

`com.adobe.appEnforced: true` が強制付与の証拠。X の「AIで作成」判定は `digitalSourceType` が根拠と考えられる。

### 1.1.1 対照実験 — Firefly を通した時だけ付与される

同一の Lightroom Classic 15.5.1・同一カメラ/レンズ・同一書き出し設定で、生成AI を使っていない
書き出し（`6x7-10_PSMS_edited.jpg`, 22,048,720 B）を比較した。

| | `6x7-18`（Firefly 使用） | `6x7-10`（生成AI未使用） |
|---|---|---|
| LrC バージョン | 15.5.1 | 15.5.1（同一） |
| カメラ / レンズ | ILCE-1 / APO-LANTHAR 110mm | 同一 |
| **APP11 (C2PA)** | 14,478 B | **セグメントごと存在しない** |
| `crs:RemoveAreas` / `fill_method` | あり（firefly） | なし |
| `xmpMM:PreservedFileName` | `6x7-18_PSMS.ARQ` | `6x7-10_PSMS.ARQ`（**両方に残る**） |

**Lightroom が常に付与するのではなく、Firefly 経路を通った時だけ付与される。**
これにより設計書 §10-3「そもそも付けない方法」が有効な対処であることが実測で裏付けられた。

また `PreservedFileName` と `DocumentID` は生成AI 未使用でも残るため、XMP のプライバシー除去
（§4.1）は C2PA の有無と独立に必要である。

### 1.2 署名は検証不能な一時 CA

```
署名者: Adobe Compliance Signer
issuer: c2pa-ephemeral-ca.local
```

トラストリスト外のローカル一時 CA。したがって:

- 検証サイト（verify.contentauthenticity.org 等）では「クレデンシャルなし」と表示される
- しかし X はトラスト状態と無関係にアサーションを読むためフラグは立つ

**この非対称性が本件の核心。** 「Content Credentials は付いていないのに AI 判定される」という一見矛盾した現象の説明になる。埃取り（Healing）に対して `compositeWithTrainedAlgorithmicMedia` が付くのは明確な過剰ラベリング。

### 1.3 外科的除去は exiftool より厳密

`exiftool -all= + -tagsfromfile` による除去と、セグメント単位の外科的除去を比較した。

| | exiftool 方式 | 外科的除去 |
|---|---|---|
| APP11/JUMBF | 削除 | 削除 |
| EXIF ブロック | **再構築**（ByteOrder II→MM 反転） | **バイト単位で無傷** |
| 埋め込みサムネイル | **消失** | 7,777 B 保持 |
| IPTC (APP13) | **改変**（digest 不整合） | 無傷・digest 整合 |
| XMP | **全消し**（dc:rights も消失） | crs 部分のみ除去、著作権保持 |
| 削除バイト数 | -46,665 | **-18,660** |
| 画素 SHA256 | 一致 | 一致 |

検証コマンドと結果は §7 に記載。**この差が imgscrub を作る技術的根拠。**

## 2. スコープ

### やること

- JPEG の C2PA/JUMBF（APP11）除去
- XMP 内のプライバシー漏洩プロパティのプロパティ単位除去
- `inspect` サブコマンドによる診断（どのレイヤーに何が入っているかの説明）
- Firefly 検出時の予防ヒント表示（「Remove の生成AI をオフにするのが本筋」）
- Lightroom Classic Export Actions 統合（書き出し後に自動実行）

### やらないこと

- PNG / TIFF / WebP / HEIC / AVIF / MP4 対応（v0.1 では明示的に skip）
- 画像のリサイズ・再圧縮・フォーマット変換
- C2PA マニフェストの生成・署名
- EXIF IFD の再構築（v0.1 では実装しない。GPS・シリアル・MakerNote の除去は v0.2。§4.2 参照）
- GUI

## 3. アーキテクチャ

```
imgscrub
├─ main.rs          CLI パース（clap）、終了コード制御
├─ jpeg/
│   ├─ segment.rs   マーカー走査イテレータ（SOI→SOS まで）
│   ├─ filter.rs    セグメント単位の保持/削除判定
│   └─ verify.rs    出力の再パース検証 + SOS バイト列同一性検証
├─ jumbf.rs         APP11 の C2PA 判定 + inspect 用アサーション読み出し（CBOR）
├─ xmp.rs           XMP のプロパティ単位除去（XML パーサ使用、正規表現は使わない）
└─ report.rs        人間向け出力（inspect / 実行サマリ）
```

依存クレート（最小）:

| クレート | 用途 | 代替不可の理由 |
|---|---|---|
| `clap` | CLI | 手書きも可能だが保守コスト |
| `quick-xml` | XMP のプロパティ単位編集 | 正規表現による XML 編集は名前空間宣言・CDATA で壊れる |
| `ciborium` | inspect 時の CBOR デコード | C2PA アサーションは CBOR |

`ciborium` と `quick-xml` は `inspect`/XMP 編集のみで使用。`--c2pa-only` の主経路は std のみで動く。

### データフロー

```
入力 JPEG
  ↓ segment.rs: マーカー走査（SOS 以降は不可侵領域として扱う）
  ↓ filter.rs:  保持/削除判定（§4.1 の表）
  ↓ xmp.rs:     保持した XMP からプロパティ除去
  ↓ 一時ファイルへ書き出し
  ↓ verify.rs:  再パース + SOS バイト列を入力と比較
  ↓ 検証通過 → atomic rename / 失敗 → 一時ファイル破棄・入力は無変更
出力 JPEG
```

## 4. 主要な技術的判断

### 4.1 allowlist はセグメント階層、denylist はプロパティ階層

セグメント単位では**保持リストに載ったものだけを通す**（未知の APPn は落とす）。保持したセグメントの内部だけは、プロパティ単位の除去リストで処理する。この二層構造を採る。

| セグメント | 既定 | 根拠 |
|---|---|---|
| APP0 (JFIF) | 保持 | 無害。解像度情報 |
| APP1 (Exif) | 保持（無編集） | 撮影データ・著作権。§4.2 |
| APP1 (XMP) | 保持（プロパティ除去） | 著作権・キーワード・タイトルが入る |
| APP2 (ICC_PROFILE) | 保持 | 色が変わるため必須 |
| **APP2 (MPF)** | **削除** | §4.3 |
| APP11 (`JP` 始まり) | **削除** | C2PA/JUMBF |
| APP13 (Photoshop/IPTC) | 保持 | 著作権・IPTC。digest 整合を保つため無編集 |
| APP14 (Adobe) | 保持 | `ColorTransform` 宣言。落とすと色解釈が変わる decoder がある |
| その他 APPn | 削除 | 未知のベンダー拡張。allowlist 思想 |
| DQT/DHT/DRI/SOF/SOS 等 | 保持（無編集） | 画像の本体 |

XMP のプロパティ除去リスト（既定）:

| プロパティ | 何が漏れるか |
|---|---|
| `crs:RemoveAreas`, `crs:fill_method` | **Firefly 使用履歴**（C2PA を消しても残る） |
| `xmpMM:PreservedFileName` | 元ファイル名（実測値 `6x7-18_PSMS.ARQ`） |
| `xmpMM:DocumentID`, `InstanceID`, `OriginalDocumentID` | カタログ横断で同一原本を追跡できる UUID |
| `xmpMM:History*`, `DerivedFrom*` | 編集履歴・派生元 |
| `dcterms:provenance` | C2PA クラウドマニフェストへの URL |

保持: `dc:*`, `photoshop:*`, `Iptc4xmpCore:*`, `Iptc4xmpExt:*`, `aux:*`, `exifEX:*`

### 4.2 既定では EXIF IFD を再構築しない

GPS 座標・`BodySerialNumber`・`LensSerialNumber`・`CameraOwnerName`・MakerNote は EXIF IFD の内部にあるため、除去には IFD 再構築（オフセット再計算）が必要になる。これを既定にすると §1.3 で示した「EXIF バイト単位無傷」という保証を失う。

**判断: 既定では触らない。検出したら警告してオプトインを促す。**

```
⚠ EXIF に GPS 座標とボディシリアルが残っています
  --strip-exif-private で除去できます（EXIF ブロックが再構築されます）
```

`--strip-exif-private` を付けた場合のみ IFD を再構築する。この二段構えにより、既定動作は常に「必要な 1 セグメントだけを落とし、他は 1 バイトも触らない」を維持する。

### 4.3 MPF (Multi-Picture Format) は削除する

APP2 の `MPF\0` セグメントには**ファイル先頭からの絶対オフセット**が格納されている。先行する APP11 を削除するとこのオフセットが全て破綻する。

選択肢は「オフセットを再計算する」か「MPF を削除する」。MPF が指すのは埋め込みプレビュー/多画像であり Web 公開時に不要なため、**削除**を採る。削除時は必ず報告する。

Lightroom 書き出しでは MPF は付かない（実測で APP2 は ICC のみ）が、カメラ出し JPEG を通した場合に問題になるため実装必須。

### 4.4 SOS 以降は不可侵領域として扱う

`0xFFDA` 以降をパースせず、バイト列としてそのままコピーする。JPEG のスキャンデータ内には `0xFF00` バイトスタッフィングや RST マーカーが現れるため、マーカー走査を継続すると誤検出する。SOS 到達時点で走査を終了する設計にする。

これにより画素の無劣化が構造的に保証される（デコード・再エンコードを一切行わないため）。

## 5. CLI インターフェース

```
imgscrub [OPTIONS] <PATH>...

サブコマンド:
  inspect <PATH>...    診断のみ。ファイルを変更しない

オプション:
  -o, --out-dir <DIR>       出力先ディレクトリ
      --in-place            入力を上書き（既定は <name>_clean.jpg を隣に作成）
      --c2pa-only           APP11 のみ除去。XMP も EXIF も一切触らない
      --strip-exif-private  GPS・シリアル・MakerNote も除去（EXIF 再構築）
      --keep <LIST>         除去リストから除外（gps, xmpmm, crs, mpf）
  -r, --recursive           ディレクトリを再帰
  -n, --dry-run             変更内容のみ表示
  -q, --quiet               サマリのみ
      --json                機械可読出力
```

既定を非破壊（別名出力）にする。`--in-place` を明示的なオプトインとする。

### 出力例

```
$ imgscrub inspect photo.jpg
photo.jpg  6750x5358  17,894,419 B

  APP11/JUMBF   14,478 B  C2PA マニフェスト
    ├ digitalSourceType: compositeWithTrainedAlgorithmicMedia
    ├ softwareAgent: Adobe Remove Object 1
    ├ com.adobe.acr.value: Healing changed, Uses GenAI
    ├ com.adobe.firefly.version: clio-erase-2.0#epoch=0-step=4000...
    ├ com.adobe.appEnforced: true      ← 書き出し設定に関係なく強制付与
    └ 署名: Adobe Compliance Signer / c2pa-ephemeral-ca.local（トラストリスト外）
  APP1/XMP      15,778 B  crs:fill_method="firefly", xmpMM:PreservedFileName
  APP1/Exif      8,855 B  Artist, Copyright（GPS なし / Serial なし）
  APP13/IPTC     8,010 B  By-line, CopyrightNotice
  APP2/ICC       3,162 B  sRGB IEC61966-2.1

  ⚠ X が「AIで作成」を付ける原因: APP11 の digitalSourceType
  ヒント: Lightroom の Remove ツールで「生成AI」をオフにすれば
          そもそも付与されません（修復ブラシ/コピースタンプも同様）

$ imgscrub photo.jpg
photo.jpg → photo_clean.jpg
  削除: APP11/JUMBF (-14,478 B), XMP: crs:RemoveAreas, crs:fill_method,
        xmpMM:PreservedFileName (-4,182 B)
  保持: Exif ICC IPTC 著作権 サムネイル（すべてバイト単位で無変更）
  画素: 無劣化（再エンコードなし）
  1 file, -18,660 B
```

## 6. Lightroom Classic 統合

`~/Library/Application Support/Adobe/Lightroom/Export Actions/` にシェルスクリプトを置くと、書き出しダイアログの「後処理」ドロップダウンに現れ、書き出したファイルが引数で渡される。実測でフォルダの存在を確認済み（空）。

`imgscrub install-lightroom-action` サブコマンドで以下を設置する。

```sh
#!/bin/sh
# imgscrub — Export Actions 用ラッパー
exec imgscrub --in-place --quiet "$@"
```

これにより手動実行がゼロになる。書き出しプリセットに紐づけられるため、Web 用プリセットにだけ適用することもできる。

## 7. エラーハンドリング方針

| 状況 | 動作 | 終了コード |
|---|---|---|
| 非対応フォーマット | `unsupported format, skipped` と表示してスキップ | 0 |
| C2PA なし・除去対象なし | `already clean` と表示 | 0 |
| MPF 検出 | 警告して MPF を削除 | 0 |
| 出力検証失敗（SOS 不一致・再パース不能） | 一時ファイル破棄、**入力は無変更** | 2 |
| 壊れた JPEG（SOI なし・切り詰め） | エラー、入力は無変更 | 2 |
| 書き込み権限なし | エラー | 2 |
| `--in-place` で既存ファイルと同名衝突 | 一時ファイル + atomic rename で回避 | — |

原則:

- **入力ファイルを壊さない。** 一時ファイルに書いて検証してから atomic rename する
- **冪等。** 2 回実行しても結果は同じ、2 回目は `already clean` で exit 0
- **Export Actions で使うため、スキップは失敗にしない。** 混在した書き出し結果でエラーにならないこと

## 8. テスト戦略

### フィクスチャ

実ファイル由来の最小 JPEG をリポジトリに含める。画素を 16x16 に縮小しつつ **APP11/XMP/EXIF は実物のまま**移植したものを使う。自分の写真なのでライセンス上の問題はない。

| フィクスチャ | 内容 |
|---|---|
| `lrc_firefly.jpg` | 本件そのもの。APP11 + crs:fill_method=firefly |
| `lrc_clean.jpg` | Firefly 未使用の LrC 書き出し（APP11 なし） |
| `camera_mpf.jpg` | MPF 付きカメラ出し JPEG |
| `no_exif.jpg` | メタデータほぼ無し |
| `truncated.jpg` | SOS が途中で切れている（異常系） |

### 不変条件テスト（全フィクスチャに対して）

1. **SOS バイト列が入力と完全一致**（画素無劣化の構造的保証）
2. 出力が再パース可能で、期待どおりのセグメント構成になっている
3. **冪等**: 2 回適用しても 1 回目と同一バイト列
4. `--c2pa-only` では APP11 以外のセグメントが全てバイト単位で一致
5. `inspect` はファイルを変更しない（前後のハッシュ一致）
6. 異常系で入力ファイルが無変更

### golden test

`inspect` の出力と `--dry-run` のセグメント一覧をスナップショットで固定する。

### CI

GitHub-hosted runner（**public リポジトリなので self-hosted は使わない** — fork PR から任意コード実行されるリスクがあるため）。

- `ubuntu-latest` / `macos-latest` で `cargo test` + `cargo clippy -- -D warnings` + `cargo fmt --check`
- タグ push で release job: macOS (aarch64/x86_64) と Linux (x86_64/aarch64) のバイナリをビルドして Releases に添付

## 9. 配布

| 経路 | 内容 |
|---|---|
| GitHub Releases | 単一バイナリ（4 ターゲット） |
| Homebrew tap | `brew install znznzna/tap/imgscrub` |
| crates.io | `cargo install imgscrub` |

**要確認**: `imgscrub` の名前が crates.io / GitHub / Homebrew core で空いているか。実装着手前にチェックする。

## 10. README の構成方針

技術的発見そのものが読み物として価値を持つため、動機セクションに実測を書く。

1. これは何か（メタデータ衛生ツール。上げる前の下ごしらえ）
2. **なぜ作ったか** — §1 の実測。強制付与と一時 CA の非対称性
3. **そもそも付けない方法を先に書く** — Remove ツールの生成AI をオフ、修復ブラシ/コピースタンプを使う
4. 使い方 / Lightroom 統合
5. 何を消して何を残すか（§4.1 の表をそのまま）
6. 何を保証するか — 画素無劣化・EXIF バイト無傷・冪等・入力を壊さない
7. C2PA について（除去は provenance の主張を失うことでもある、という注記）

「剥がす前に、そもそも付けない方法」を先に置くことで、ツールの立ち位置を誠実にする。

## 11. 確定事項と残件

### 確定（2026-09-07）

| 項目 | 決定 |
|---|---|
| フレーミング | F2 書き出し衛生（allowlist 思想） |
| 対応フォーマット | JPEG のみ。他は非破壊スキップ |
| 名前 | `imgscrub`（下記のとおり全経路で空き確認済み） |
| 言語 / 配布 | Rust 単一バイナリ。GitHub Releases + Homebrew tap + crates.io |
| CI | GitHub-hosted（public リポなので self-hosted 不可） |
| EXIF 内プライバシー情報 | **検出して警告・オプトイン。既定では EXIF を触らない** |
| `--strip-exif-private` | **v0.2 に回す**（IFD 再構築は v0.1 に含めない） |
| LrC Export Actions 統合 | v0.1 に含める |

名前の空き確認結果:

| | crates.io | Homebrew core | github.com/znznzna |
|---|---|---|---|
| `imgscrub` | 空き (404) | 空き (404) | 空き (404) |

GitHub 全体に `aliffattahfaiz/imgscrub` が 1 件存在するが、リポジトリ名はアカウント名前空間なので実害なし。

この決定により **v0.1 は IFD 再構築を一切含まない**。したがって §1.3 の「EXIF バイト単位で無傷」は
v0.1 の全動作モードで成立する不変条件となり、テスト（§8 不変条件 4）で機械的に固定できる。

### 残件

1. `inspect` の CBOR デコードをどこまで作るか（全アサーション展開 vs 既知フィールドのみ）
   - 暫定方針: 既知フィールド優先。`--json` では生の CBOR を JSON に素通しする
2. リポジトリを公開する GitHub アカウント（`znznzna` 想定・要確認）
3. Homebrew tap リポジトリ `znznzna/homebrew-tap` の新規作成

## 12. 検証済みコマンドと実測値（付録）

> 以下は設計時点（Python PoC / exiftool）の実測値。実装後の imgscrub による実測は
> 実装計画の Phase 2-3 の記録を参照（既定モードで -24,147 B。xpacket パディング約 4KB と
> `xmpMM:History` / `DerivedFrom` も落とすため PoC より削除量が多い）。

```
元ファイル: 6x7-18_PSMS_edited.jpg      17,894,419 B
外科的除去:                              17,875,759 B  (-18,660)
exiftool 方式:                           17,847,754 B  (-46,665)

画素 SHA256（デコード後）: 7788fcf8c4c542f3e9c6f033852ef9a0  ← 3 ファイルすべて一致

外科的除去後の検証:
  AI 痕跡 grep (c2pa|jumbf|firefly|genai|remove|algorithmic|digitalsource): 0 件
  ExifByteOrder:      Little-endian (Intel, II)   ← 元と同一
  ThumbnailLength:    7777                        ← 保持
  CurrentIPTCDigest:  018f31d057249cb2eca08437c4789e2e
  IPTCDigest:         018f31d057249cb2eca08437c4789e2e  ← 整合
  XMP-dc:Rights:      Motoki Endo                 ← 保持
```
