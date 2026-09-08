# imgscrub

JPEG から C2PA と追跡用メタデータを取り除く。**画素は一切触らない。**

[English](README.md)

```
$ imgscrub photo.jpg
photo.jpg → photo_clean.jpg
  削除: APP11 (C2PA マニフェスト) (-14478 B)
  削除: APP1/XMP (7 プロパティ + パディング) (-9669 B)
        xmpMM:PreservedFileName — 元ファイル名
        crs:RemoveAreas — 生成AI消しゴムの適用履歴
        ...
  17894419 B → 17870272 B (-24147 B)
```

## なぜ作ったか

フィルムスキャンの埃を Lightroom Classic で数点消し、Content Credentials を**オフ**にして
JPEG を書き出したら、X で「AIで作成」のラベルが付いた。

実バイトを読んで分かったことが 3 つある。

### 1. C2PA マニフェストは設定に関係なく付与される

書き出したファイルの `APP11` セグメント（14,478 バイト）に、完全な C2PA マニフェストが
入っていた。

| フィールド | 値 |
|---|---|
| `softwareAgent.name` | Adobe Remove Object |
| `com.adobe.acr.value` | Healing changed, **Uses GenAI** |
| `com.adobe.firefly.version` | `clio-erase-2.0#epoch=0-step=4000...` |
| `digitalSourceType` | **`…/compositeWithTrainedAlgorithmicMedia`** |
| `com.adobe.appEnforced` | **true** |

最後のフィールドが答えを書いている。書き出し設定に関係なくアプリが強制付与する。
プラットフォームが AI ラベルの根拠にしているのは、ほぼ確実に `digitalSourceType`。

埃を数点消すことは、見る人が受け取る意味での「AIで作成」ではない。それでも IPTC の語彙は、
完全に生成された画像と同じ用語を割り当ててくる。

### 2. 一時 CA で署名されているので、検証サイトは「クレデンシャルなし」と言う

```
署名者: Adobe Compliance Signer
発行者: c2pa-ephemeral-ca.local     ← どのトラストリストにも無い
```

だから検証サイトは「Content Credentials は見つかりません」と表示する。にもかかわらず
プラットフォームはアサーションを読んでラベルを付ける。この非対称が、外から見て矛盾して
見える理由であり、生バイトを読むまで分からなかった理由でもある。

### 3. C2PA を消すだけでは足りない

マニフェストを消しても、XMP の `crs:fill_method="firefly"` と `crs:RemoveAreas` は生き残る。
Lightroom はさらに `xmpMM:PreservedFileName`（元ファイル名。私の場合 `6x7-18_PSMS.ARQ`）と、
同一の原本から書き出した全ファイルを突き合わせられる UUID を書き込む。
**これらは生成AI を使ったかどうかに関係なく付く。**

## 剥がす前に読んでほしいこと

**まだ書き出していないなら、元から直すほうがいい。**

1. **Lightroom の Remove ツールで「生成AI」をオフにする。** 従来のコンテンツ認識塗りつぶしは
   Firefly を通らないので、マニフェストが生成されない
2. **または修復ブラシ / コピースタンプを使う。** これらは生成AI 経路に一切乗らない

対照実験で確認した。同じ Lightroom バージョン・同じカメラとレンズ・同じ書き出し設定で、
生成AI を使わずに埃を消した場合 → **`APP11` セグメントが丸ごと存在しない**。
Lightroom が常に付与するのではなく、Firefly を通った時に付与している。

事後に記録を消すことは、狭い意味では真実だったこと（Firefly は実際に動いた）を隠すことでも
ある。埃取りに Firefly を使わないほうが誠実な解決で、`imgscrub inspect` は Firefly の痕跡を
見つけたらそう言う。剥がす機能は、すでに書き出してしまったファイルと、AI とは何の関係もない
追跡メタデータのために使ってほしい。

## 何を消して何を残すか

セグメント階層は allowlist で、載っているものだけを通す。通したセグメントの内部は、
プロパティ単位で名前を指定して除去する。

| セグメント | 既定 | 理由 |
|---|---|---|
| `APP0` JFIF | 保持 | 無害な解像度情報 |
| `APP1` Exif | **保持・無編集** | 撮影データ、著作者、著作権 |
| `APP1` XMP | 保持・プロパティ除去 | 著作権と追跡情報が同居している |
| `APP2` ICC | 保持 | 無いと色が変わる |
| `APP2` MPF | **削除** | ファイル先頭からの絶対オフセットが破綻する |
| `APP11` JUMBF | **削除** | C2PA マニフェスト |
| `APP13` Photoshop/IPTC | **保持・無編集** | IPTC、著作権 |
| `APP14` Adobe | 保持 | `ColorTransform` を宣言。落とすと色解釈が変わる |
| その他の `APPn` | 削除 | 未知のベンダー拡張 |
| `DQT`/`DHT`/`SOF`/`SOS` 等 | 保持・無編集 | 画像そのもの |

既定で除去する XMP プロパティ:

| プロパティ | 何が漏れるか |
|---|---|
| `crs:RemoveAreas`, `crs:fill_method` | 生成AI消しゴムの使用 |
| `xmpMM:PreservedFileName` | 元ファイル名 |
| `xmpMM:DocumentID`, `InstanceID`, `OriginalDocumentID` | 書き出しを突き合わせられる UUID |
| `xmpMM:History`, `DerivedFrom` | 編集履歴、派生元 |
| `dcterms:provenance` | クラウド上の C2PA マニフェストへのリンク |

保持: `dc:*`, `photoshop:*`, `Iptc4xmpCore:*`, `Iptc4xmpExt:*`, `aux:*`, `exifEX:*`、
および `crs:*` の残り。

### GPS とシリアル番号は検出するが除去しない（v0.1）

GPS 座標・`BodySerialNumber`・`LensSerialNumber`・`CameraOwnerName`・`MakerNote` は
Exif IFD の**内部**にある。除去するには IFD を再構築してオフセットを再計算する必要があり、
下記の「Exif がバイト単位で無変更」という保証を失う。

v0.1 は検出して知らせるだけ。除去は v0.2 の `--strip-exif-private` で入れる。

## 何を保証するか

以下はすべて CI で走るテストとして機械的に固定してある。

1. **スキャンデータが入力とバイト単位で一致する。** 画素はデコードすらしないので、
   「無劣化」は主張ではなく構造的な事実（コードに再エンコード経路が存在しない）
2. **Exif・ICC・IPTC はバイト単位で無変更。** バイトオーダー、埋め込みサムネイル、
   IPTC digest がすべて保たれる。（`exiftool -all=` は Exif を再構築してバイトオーダーを
   反転させ、サムネイルを落とす。この差が、このツールを作った理由）
3. **冪等。** 2 回実行すると同じバイト列になり、`already clean` と報告する
4. **入力を壊さない。** 一時ファイルに書き、検証を通してから atomic rename する。
   検証に失敗した場合は何も変更されない

## インストール

```sh
brew install znznzna/tap/imgscrub
# または
cargo install imgscrub
```

[Releases](https://github.com/znznzna/imgscrub/releases) からバイナリを取ってもいい。

## 使い方

```
imgscrub [OPTIONS] <PATH>...
imgscrub inspect <PATH>...
imgscrub install-lightroom-action
```

| オプション | 効果 |
|---|---|
| `-o, --out-dir <DIR>` | このディレクトリに書く |
| `--in-place` | 入力を上書き（既定は隣に `<name>_clean.jpg` を作る） |
| `-n, --dry-run` | 何が除去されるかを表示するだけ。ファイルを書かない |
| `--c2pa-only` | `APP11` のみ除去。XMP も未知の `APPn` も触らない |
| `--keep <LIST>` | 除去対象から外す: `xmpmm`, `crs`, `mpf`, `unknown` |
| `-r, --recursive` | ディレクトリを辿る |
| `-q, --quiet` | サマリのみ |
| `--json` | 機械可読な出力 |

終了コードは成功で `0`。スキップしたファイルや already clean も `0` を返すのでパイプラインで
安全に使える。処理できなかったファイルがあった場合のみ `2`。

> **`--in-place` は取り消せない。** 既定は非破壊で、入力には触らず隣に `<name>_clean.jpg` を
> 作る。`--in-place` は入力を置き換え、除去したメタデータを戻す手段はない。
> まず `-n` で何が消えるかを確認し、再書き出しできないファイルはコピーを取っておくこと。
>
> どちらのモードでも画素は一切触らないので、上書きしたファイルも画質はそのままである。
> 失われるのはメタデータだけ。

### まず診断する

`inspect` は、どのレイヤーに何が入っていて、なぜプラットフォームがフラグを立てるのかを
説明する。

```
$ imgscrub inspect photo.jpg
photo.jpg  6750x5358  17894419 B

  APP11/JUMBF       14478 B  C2PA マニフェスト
  APP1/Exif          8855 B  撮影データ・著作権
  APP13/IPTC         8010 B  IPTC・Photoshop リソース
  APP2/ICC           3162 B  カラープロファイル
  APP1/XMP          15778 B  xmpMM:DocumentID, ..., crs:RemoveAreas
  APP14/Adobe          16 B  ColorTransform 宣言

  C2PA マニフェスト: urn:c2pa:e7a6bef0-...:adobe
    生成元: Adobe Lightroom Classic 15.5.1（C2PA 仕様 2.4.0）
    アクション: c2pa.edited
      ツール: Adobe Remove Object 1
      digitalSourceType: compositeWithTrainedAlgorithmicMedia
      com.adobe.acr.value: Healing changed, Uses GenAI
    com.adobe.appEnforced: true  ← 書き出し設定に関係なく強制付与
    署名: Adobe Compliance Signer
      発行者: c2pa-ephemeral-ca.local
      → 一時 CA。トラストリスト外

  ⚠ X が「AIで作成」を付ける原因: APP11 の digitalSourceType
```

### Lightroom の書き出し後に自動実行する

```sh
imgscrub install-lightroom-action
```

書き出しダイアログの「後処理」で **imgscrub** を選ぶと、書き出しごとに自動で走る。
Web 用プリセットにだけ紐づけることもできる。実行のたびに
`~/Library/Logs/imgscrub-lightroom.log` に記録されるので、動いたかどうかを確認できる。
`uninstall-lightroom-action` で外せる。

これを動かすには 2 つ条件があり、どちらも間違えやすい。

- **シェルスクリプトではなくアプリケーションバンドルである必要がある。** Lightroom は
  アイテムを LaunchServices 経由で開くため、`.sh` は `error -10811`
  （`kLSNotAnApplicationErr`）で拒否される。`install-lightroom-action` は `osacompile` で
  AppleScript のドロップレットを生成し、書き出したファイルを Apple Event の `odoc` として
  受け取れるようにする
- **imgscrub は絶対パスで呼ぶ。** GUI アプリはシェルの `PATH` を継承しないので
  `/opt/homebrew/bin` は入っていない。インストーラは絶対パスを埋め込むが、Cellar の
  バージョン入りパスではなく `/opt/homebrew/bin` の symlink を優先する
  （前者だと次の `brew upgrade` で壊れる）

**登録したら Lightroom を再起動する。** `Export Actions` フォルダは起動時にしか
読まれないので、後から置いたものはドロップダウンに出ない。

**書き出しプリセットは後処理を絶対パスで保存する。** 存在しないものを指したプリセットは、
Lightroom が何も実行せず何も言わない状態になる。`install-lightroom-action` は
プリセットを走査して該当するものの名前を出す。`--fix-presets` で書き換えられる
（`.imgscrub-backup` に元を残す）。

## 対応範囲

JPEG のみ。PNG・TIFF・WebP・HEIC・AVIF・MP4 も C2PA を運べるが、v0.1 では
`unsupported format, skipped` と表示して触らない。

## C2PA について

C2PA は妥当な発想で、このツールはその一部に逆行している。マニフェストを消すことは、
そのファイルが自分について主張できること（本物のカメラで撮られた、あなたのものである）も
同時に消す。プラットフォームが「検証済みクレデンシャルあり」を肯定的なシグナルとして
表示し始めたら、何も主張しないファイルを出すより、生成ツールを使わずに撮って現像し、
正規に署名されたクレデンシャルを付ける方が筋がいい。

このツールが異議を唱えているのはもっと狭い範囲だ。利用者が表明した設定を越えて付与され、
検証できない証明書で署名され、写真の作られ方について見る人に誤ったことを伝える語彙が
使われている、という点。

## ライセンス

MIT
