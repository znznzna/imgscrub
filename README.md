# imgscrub

Strip C2PA/AI-provenance and tracking metadata from JPEGs — **without touching a single pixel**.

[日本語版 / Japanese](README.ja.md)

```
$ imgscrub photo.jpg
photo.jpg → photo_clean.jpg
  removed: APP11 (C2PA manifest) (-14478 B)
  removed: APP1/XMP (7 properties + padding) (-9669 B)
        xmpMM:PreservedFileName — original filename
        crs:RemoveAreas — generative-fill edit history
        ...
  17894419 B → 17870272 B (-24147 B)
```

## Why this exists

I removed a few dust spots from a film scan in Lightroom Classic, exported a JPEG with
Content Credentials **turned off**, and X labeled the photo "Made with AI".

Digging into the actual bytes turned up three things worth knowing.

### 1. The C2PA manifest is attached whether you want it or not

The export carried a complete C2PA manifest in an `APP11` segment (14,478 bytes), holding:

| Field | Value |
|---|---|
| `softwareAgent.name` | Adobe Remove Object |
| `com.adobe.acr.value` | Healing changed, **Uses GenAI** |
| `com.adobe.firefly.version` | `clio-erase-2.0#epoch=0-step=4000...` |
| `digitalSourceType` | **`…/compositeWithTrainedAlgorithmicMedia`** |
| `com.adobe.appEnforced` | **true** |

That last field says it plainly: the app attaches this regardless of the export setting.
The `digitalSourceType` is almost certainly what platforms read to apply an AI label.

Healing a few dust specks is not "made with AI" in any sense a viewer would recognize,
but the IPTC vocabulary term is the same one used for genuinely synthetic imagery.

### 2. It is signed by an ephemeral CA, so verifiers say there is no credential

```
Signer: Adobe Compliance Signer
Issuer: c2pa-ephemeral-ca.local     ← not in any trust list
```

So verification sites report "no Content Credentials found", while platforms still read
the assertions and apply the label. That asymmetry is why the situation looks
contradictory from the outside — and why it took reading the raw bytes to understand.

### 3. Stripping C2PA is not enough

The `crs:fill_method="firefly"` and `crs:RemoveAreas` properties survive in XMP even
after the manifest is gone. Lightroom also writes `xmpMM:PreservedFileName` (your
original filename — `6x7-18_PSMS.ARQ` in my case) and UUIDs that let anyone correlate
every export from the same original. **Those appear whether or not you used AI tools.**

## Read this before you strip anything

**If you have not exported yet, fix it at the source instead:**

1. **Turn off "Generative AI" in Lightroom's Remove tool.** The classic content-aware
   fill does not go through Firefly and no manifest is generated.
2. **Or use the Healing brush / Clone stamp**, which never touch the generative path.

I verified this with a control: same Lightroom version, same camera and lens, same
export settings, dust removed without generative AI → **no `APP11` segment at all**.
Lightroom does not always attach a manifest; it attaches one when Firefly was involved.

Removing the record after the fact hides something that was, narrowly, true — Firefly
did run. Not using Firefly for dust removal is the honest fix, and `imgscrub inspect`
will tell you so when it sees Firefly traces. Use the stripping for files you already
exported, and for the tracking metadata that has nothing to do with AI at all.

## What it does and does not remove

Segment level is an allowlist: only what is listed survives. Inside surviving segments,
properties are removed by name.

| Segment | Default | Why |
|---|---|---|
| `APP0` JFIF | keep | harmless resolution info |
| `APP1` Exif | **keep, unedited** | camera data, author, copyright |
| `APP1` XMP | keep, properties removed | holds both copyright and tracking data |
| `APP2` ICC | keep | colors change without it |
| `APP2` MPF | **remove** | contains absolute file offsets that break |
| `APP11` JUMBF | **remove** | the C2PA manifest |
| `APP13` Photoshop/IPTC | **keep, unedited** | IPTC, copyright |
| `APP14` Adobe | keep | declares `ColorTransform`; dropping it changes color handling |
| any other `APPn` | remove | unknown vendor extension |
| `DQT`/`DHT`/`SOF`/`SOS`… | keep, unedited | the image itself |

XMP properties removed by default:

| Property | What it leaks |
|---|---|
| `crs:RemoveAreas`, `crs:fill_method` | generative-fill usage |
| `xmpMM:PreservedFileName` | your original filename |
| `xmpMM:DocumentID`, `InstanceID`, `OriginalDocumentID` | UUIDs that correlate exports |
| `xmpMM:History`, `DerivedFrom` | edit history, parent asset |
| `dcterms:provenance` | link to a cloud C2PA manifest |

Kept: `dc:*`, `photoshop:*`, `Iptc4xmpCore:*`, `Iptc4xmpExt:*`, `aux:*`, `exifEX:*`, and
the rest of `crs:*`.

### GPS and serial numbers are detected, not removed (v0.1)

GPS coordinates, `BodySerialNumber`, `LensSerialNumber`, `CameraOwnerName` and
`MakerNote` live *inside* the Exif IFD. Removing them means rebuilding the IFD and
recomputing offsets, which would forfeit the byte-for-byte Exif guarantee below.

v0.1 detects them and tells you. Removal lands in v0.2 behind `--strip-exif-private`.

## What it guarantees

Every one of these is enforced by a test that runs in CI:

1. **The scan data is byte-identical to the input.** Pixels are never decoded, so
   "lossless" is structural rather than a claim — there is no re-encode path in the code.
2. **Exif, ICC and IPTC come out byte-identical.** Byte order, the embedded thumbnail and
   the IPTC digest all survive. (`exiftool -all=` rebuilds Exif, flips byte order, and
   drops the thumbnail; that is the difference this tool exists to make.)
3. **Idempotent.** Running twice produces the same bytes and reports `already clean`.
4. **Your input is never damaged.** Output goes to a temp file, gets verified, and only
   then is atomically renamed. A failed verification leaves everything untouched.

## Install

```sh
brew install znznzna/tap/imgscrub
# or
cargo install imgscrub
```

Or grab a binary from [Releases](https://github.com/znznzna/imgscrub/releases).

## Usage

```
imgscrub [OPTIONS] <PATH>...
imgscrub inspect <PATH>...
imgscrub install-lightroom-action
```

| Option | Effect |
|---|---|
| `-o, --out-dir <DIR>` | write into this directory |
| `--in-place` | overwrite the input (default writes `<name>_clean.jpg` alongside) |
| `--c2pa-only` | remove `APP11` only; do not touch XMP or unknown `APPn` |
| `--keep <LIST>` | exclude from removal: `xmpmm`, `crs`, `mpf`, `unknown` |
| `-r, --recursive` | walk directories |
| `-n, --dry-run` | report without writing |
| `-q, --quiet` | summary only |
| `--json` | machine-readable output |

Exit code is `0` for success — including files that were skipped or already clean, so it
is safe in a pipeline — and `2` when a file could not be processed.

### Diagnose first

`inspect` explains which layer holds what, and why a platform would flag the file:

```
$ imgscrub inspect photo.jpg
photo.jpg  6750x5358  17894419 B

  APP11/JUMBF       14478 B  C2PA manifest
  APP1/Exif          8855 B  camera data, copyright
  APP13/IPTC         8010 B  IPTC, Photoshop resources
  APP2/ICC           3162 B  color profile
  APP1/XMP          15778 B  xmpMM:DocumentID, ..., crs:RemoveAreas
  APP14/Adobe          16 B  ColorTransform declaration

  C2PA manifest: urn:c2pa:e7a6bef0-...:adobe
    generator: Adobe Lightroom Classic 15.5.1 (C2PA spec 2.4.0)
    action: c2pa.edited
      tool: Adobe Remove Object 1
      digitalSourceType: compositeWithTrainedAlgorithmicMedia
      com.adobe.acr.value: Healing changed, Uses GenAI
    com.adobe.appEnforced: true  ← attached regardless of export settings
    signature: Adobe Compliance Signer
      issuer: c2pa-ephemeral-ca.local
      → ephemeral CA, outside every trust list

  ⚠ this file will be labeled "Made with AI" because of digitalSourceType
```

### Run it automatically after every Lightroom export

```sh
imgscrub install-lightroom-action
```

Pick **imgscrub** under "Post-Processing" in the export dialog and it runs on every
export — attach it to just your web preset if you like. Every run is logged to
`~/Library/Logs/imgscrub-lightroom.log` so you can confirm it fired.
`uninstall-lightroom-action` removes it.

Two things make this work, and both are easy to get wrong:

- **It must be an application bundle, not a shell script.** Lightroom opens the item
  through LaunchServices, which refuses a `.sh` with `error -10811`
  (`kLSNotAnApplicationErr`). `install-lightroom-action` builds an AppleScript droplet
  with `osacompile` so it can receive the exported files as an `odoc` Apple Event.
- **imgscrub is called by absolute path.** GUI apps do not inherit your shell `PATH`, so
  `/opt/homebrew/bin` is not on it. The installer embeds an absolute path — preferring the
  stable `/opt/homebrew/bin` symlink over the versioned Cellar path, which would break on
  the next `brew upgrade`.

**After installing, restart Lightroom.** The `Export Actions` folder is only read at
launch, so a freshly installed action will not appear in the dropdown until you do.

**Export presets store the post-processing action as an absolute path.** If a preset
points at something that no longer exists, Lightroom runs nothing and says nothing.
`install-lightroom-action` scans your presets and names the stale ones; `--fix-presets`
rewrites them (keeping a `.imgscrub-backup` copy).

## Scope

JPEG only. PNG, TIFF, WebP, HEIC, AVIF and MP4 can all carry C2PA, but they are reported
as `unsupported format, skipped` and left alone.

## A note on C2PA

C2PA is a reasonable idea and this tool works against part of it. Removing a manifest
also removes any claim the file could make *for* itself — that it came from a real camera,
that it is yours. If platforms start surfacing "has verified credentials" as a positive
signal, the better path is to shoot and edit without the generative tools and keep a
properly signed credential, rather than to ship a file that asserts nothing.

What this tool objects to is narrower: a disclosure attached over the user's stated
preference, signed by a certificate that cannot be verified, using a vocabulary term that
tells viewers something false about how the picture was made.

## License

MIT
