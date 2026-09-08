# OpenDocScan Implementation Plan

> **For agentic workers:** Section 8 is the only part of this plan that's bite-sized/TDD-ready for direct execution (M0 kickoff). Milestones M1+ each span independent subsystems (Rust core, FFI bridge, Flutter UI, per-platform build) — draft a dedicated task-by-task plan per milestone with `superpowers:writing-plans` when that milestone starts, rather than executing this document task-by-task end to end.

**Goal:** Ship an open-source document scanner for iOS and Android that does everything CamScanner does for scanning/export — capture, auto edge-detect, perspective-correct, filter, multi-page PDF, OCR — 100% on-device, with no ads, accounts, watermarks, or paywalls.

**Architecture:** Flutter owns camera capture, file import, permissions, previews, and sharing. A Rust workspace ("the core") owns all image processing, PDF export, OCR, and the local document library, exposed to Flutter via `flutter_rust_bridge`. The core has zero mobile-specific code — it builds and is fully unit-tested as an ordinary `cargo test` target on desktop/CI, with only the bridge glue crate touching mobile toolchains.

**Tech stack:** Flutter 3.x + Dart; Rust (stable channel); `flutter_rust_bridge` v2 with `cargokit` for build integration; `image` + `imageproc` for vision; `printpdf` for export; `rusqlite` (bundled SQLite + FTS5) for the local library/search index; `tract` (pure-Rust ONNX runtime) for offline OCR.

## Global Constraints

These apply to every task and milestone below, copied verbatim from the brief:

- 100% local processing — no cloud upload, no network calls anywhere in the core scanning pipeline.
- No ads, no account, no watermark, no paywall for core scanning/export.
- Rust core must be platform-agnostic and testable on desktop/CI without a mobile toolchain.
- Flutter layer handles only camera, file picker, permissions, previews, sharing.
- Rust layer handles image processing, correction, filters, PDF export, OCR, local storage.
- Lightweight dependencies; prefer permissive (MIT/Apache-2.0) licenses.
- Minimize native dependency complexity (favor pure-Rust crates over ones that shell out to C/C++ libraries).
- Design for future desktop support (Tauri, matching this monorepo's other apps) without a rewrite.

## Assumptions (stated per the brief's instruction — flag before proceeding if any are wrong)

1. **Product name** "OpenDocScan," matching the directory name and this monorepo's naming convention (OpenPhotoId, OpenScreenshot, OpenPDFEdit).
2. **License**: dual MIT/Apache-2.0, matching every other app in this monorepo.
3. **Milestone cadence**: "2–3 week milestones" means each milestone below is *sized* to roughly 2–3 weeks for one engineer, not that the entire MVP ships in 2–3 weeks total. Seven milestones (M0–M6) are proposed.
4. **"Import from files"** means importing existing image files (JPEG/PNG/HEIC); importing an existing PDF to re-edit is out of MVP scope (that's OpenPDFEdit's job, not this app's).
5. **OCR default**: on by default per document (bundled model, no download), with a settings toggle to disable it for speed on low-end devices — never phones home to fetch anything.
6. **Minimum OS versions**: Android minSdk 24 (covers CameraX reliably, ~98%+ of active devices), iOS 15+. Revisit against current Flutter-supported floors at implementation time.
7. **No analytics/crash-reporting SDK** in v1 — any telemetry dependency would contradict the "100% local" promise; this is a hard dependency-policy rule, not just a default.

---

## 1. PRD-lite

**Problem.** Existing mobile scanner apps (CamScanner and clones) are built around subscription paywalls, intrusive ads, forced accounts, and cloud sync that has caused real users to lose documents when sync breaks or an account is suspended. None of that is inherent to "photograph a document and get a clean PDF" — it's monetization architecture bolted onto a fundamentally simple, local operation.

**Users.**
- Students/professionals scanning receipts, notes, forms, and IDs who want a fast, private, no-nonsense tool.
- Privacy-conscious users who won't upload personal documents (tax forms, medical records, contracts) to a third party's cloud, ever.
- Users who've been burned by a scanner app losing their documents, hitting a paywall mid-task, or crashing.

**MVP (from brief).** Camera capture + import; automatic edge detection with user-adjustable corners; perspective correction; multi-page PDF export; B/W, brightness, and enhancement filters; optional offline OCR; a local library so documents are never lost; local share/export; a deterministic test pipeline.

**Non-goals for v1** (explicit, to keep scope honest): cloud sync/backup, collaborative sharing, any subscription/premium tier, e-signatures, business-card mode, batch import beyond a manual multi-select, desktop app (architecturally prepared for, not built), languages beyond an initial OCR set (assume English + one or two others — pick at OCR-model-selection time based on what the chosen model ships pretrained).

**Success metrics.**
- Time from "tap capture" to "corrected page ready for review" under 1.5s on a mid-tier device (2022-era Android, e.g. Snapdragon 6-series).
- Edge-detection corner IoU ≥ 0.9 against ground truth on the labeled test dataset (§6).
- Zero silent document loss: every saved page/document survives an app kill, force-quit, and reinstall-with-data-restore across the test suite; library integrity self-check runs on every app start.
- Crash-free session rate ≥ 99.5% in beta (measured via the OS's own crash reporting in store consoles — not a bundled telemetry SDK).
- Core Rust library adds < 15 MB to the release APK/IPA per architecture slice; OCR models add < 15 MB combined.
- 100% of scans/exports work with the device in airplane mode (proves the "no network calls" constraint empirically, not just by code review).

## 2. Architecture

```
┌─────────────────────────── Flutter (Dart) ───────────────────────────┐
│  Capture screen (camera preview, live guide overlay, shutter)        │
│  Import screen (file_picker/image_picker multi-select)               │
│  Crop/adjust screen (draggable corner handles, seeded by Rust)       │
│  Filter screen (B/W · brightness · "enhance" preview)                │
│  Page tray (reorder / add / delete pages before export)              │
│  Library screen (grid, OCR-text search, sort/delete)                 │
│  Viewer/share screen (share_plus → OS share sheet)                   │
└───────────────────────────────┬────────────────────────────────────-─┘
                                 │ flutter_rust_bridge (generated Dart API)
┌────────────────────────────────▼──────────────────────────────────────┐
│                         Rust core (cargo workspace)                    │
│  docscan-core    image IO, shared types, error type                   │
│  docscan-detect  document-quad detection (edges → contours → quad)    │
│  docscan-transform  perspective warp (imageproc::geometric_transforms) │
│  docscan-filters B/W · brightness/contrast · adaptive "enhance"       │
│  docscan-ocr     OCR engine trait; tract/ONNX backend (feature-gated) │
│  docscan-pdf     multi-page PDF assembly (printpdf), deterministic    │
│  docscan-store   rusqlite-backed library: pages, documents, FTS5 index│
│  docscan-ffi     thin flutter_rust_bridge glue only — no logic here   │
└─────────────────────────────────────────────────────────────────────-─┘
```

**Data flow.** Capture or import produces raw image bytes in Flutter → passed once across the bridge to `docscan-detect::find_document_quad` → Flutter overlays the returned quad as draggable handles for user confirmation/adjustment → confirmed quad + image go to `docscan-transform::warp_to_quad` → `docscan-filters` applies the chosen filter → the resulting page image is added to an in-progress document held by `docscan-store` (staging area, not yet in the permanent library) → repeat per page → on export, `docscan-pdf` composes all staged pages into one PDF (running `docscan-ocr` per page first if enabled, embedding an invisible text layer) → the finished PDF is written to the library and handed back to Flutter for `share_plus`.

**Bridge surface is coarse-grained by design**: whole images and whole documents cross the FFI boundary, never per-pixel or per-frame chatty calls. This keeps marshalling simple and testable and avoids a common source of subtle FFI bugs.

**OCR engine seam.** `docscan-ocr` defines an `OcrEngine` trait with a single tract/ONNX-backed implementation for v1. This mirrors a pattern already proven in this monorepo's OpenPhotoId app (`frame-engine`'s `EngineSession` seam, swapped between `ort` desktop and `tract` mobile backends via mutually exclusive Cargo features) — applying the same shape here means a future desktop build could add a heavier OCR backend later without touching call sites.

**Local library.** `docscan-store` keeps original per-page images, the assembled PDF, and OCR text in a `rusqlite` database (bundled SQLite, FTS5 for full-text search across scanned documents) under the app's documents directory. An integrity self-check runs at startup (verify every DB row's referenced file still exists; quarantine orphans instead of silently dropping them) — this is the concrete, testable answer to "documents are never lost."

**Future desktop.** No crate above touches a mobile-only API; the *only* mobile-specific code is `docscan-ffi`. A later desktop app (Tauri, matching OpenPhotoId/OpenPDFEdit) reuses every other crate unchanged and only needs a new thin FFI/command layer — same shape this monorepo already uses twice.

## 3. Recommended stack

**Flutter packages:**

| Package | Purpose | License |
|---|---|---|
| `camera` | Manual-capture live preview (CameraX-backed on Android) — no smart-capture/ML Kit dependency | BSD-3 |
| `file_picker` | Import existing image files | MIT |
| `permission_handler` | Camera/photos permission requests | MIT |
| `share_plus` | Hand the exported PDF to the OS share sheet | BSD-3 |
| `path_provider` | Resolve the app documents directory | BSD-3 |
| `riverpod` | State management / DI (testable via provider overrides) | MIT |
| `flutter_rust_bridge` | Generated Dart bindings to the Rust core | MIT |

The live capture-screen guide rectangle is produced by throttled calls (every 200–300ms) to the same `find_document_quad` Rust function used on the final photo — deliberately avoiding Google ML Kit or any other native vision SDK, which would violate "minimize native dependency complexity" and complicate the offline story.

**Rust crates:**

| Crate | Purpose | License | Note |
|---|---|---|---|
| `image` | Decode/encode JPEG/PNG, pixel buffers | MIT/Apache-2.0 | pure Rust |
| `imageproc` | Canny edges, contours, `geometric_transformations::Projection`/`warp` for perspective correction | MIT/Apache-2.0 | pure Rust; confirmed it exposes a 4-point projective warp — exactly what perspective correction needs, no separate homography math library required |
| `printpdf` | Multi-page PDF assembly, embeds JPEG/PNG per page + a text layer | MIT | pure Rust; note its images-are-per-page-resource model when composing — fine since each page has exactly one image |
| `rusqlite` (bundled feature) | Local library DB + FTS5 search | MIT | bundles SQLite C source but manages it internally — no system dependency to cross-compile ourselves |
| `tract` / `tract-onnx` | Pure-Rust ONNX inference for OCR | MIT/Apache-2.0 | already validated for Android/iOS cross-compilation in this monorepo's OpenPhotoId app (`ort` has no prebuilt mobile binaries; `tract` does not need one) |
| `serde`/`serde_json`, `thiserror` | Serialization, error types | MIT/Apache-2.0 | |

**OCR model**: target a compact PaddleOCR-style detector+recognizer (DBNet + SVTR/CRNN) exported to ONNX, run through `tract`. A pure-Rust crate doing exactly this (`pure-onnx-ocr`, Apache-2.0, built on `tract-onnx`) already exists but is early-stage (proof-of-concept level as of its own late-2025 status) — treat it as a reference implementation/starting point to vendor or fork from, not a drop-in dependency to pin blindly. Verify PaddleOCR weight licensing (Apache-2.0 upstream) before bundling. Full risk discussion in §7.

**Bridge approach**: `flutter_rust_bridge` v2 with the `cargokit` integration backend (the current default — it wires cargo into Gradle/CocoaPods/CMake and auto-provisions the Android NDK and cross targets). `flutter_rust_bridge`'s newer "native assets" backend is flagged upstream as a possible future direction; stick with `cargokit` for v1 since it's the proven, documented path, and revisit if native assets becomes the stable default before this ships.

## 4. Mobile build plan

**Android.**
- Targets: `aarch64-linux-android` (primary, real devices), `armv7-linux-androideabi` (older devices), `x86_64-linux-android` (emulator).
- `cargokit` auto-installs the NDK and these Rust targets as part of the Gradle build; no manual per-arch scripting needed.
- Package as an Android App Bundle (not a universal APK) so Play delivers only the needed ABI's `.so`, keeping per-device download size down; strip symbols in release builds.
- Permissions: `CAMERA`, and either `READ_MEDIA_IMAGES` (API 33+) or `READ_EXTERNAL_STORAGE` (below it) for import — `permission_handler` abstracts the split.
- minSdkVersion 24 (assumption #6 above), targetSdkVersion = latest stable at build time.
- Signing/publishing to Play is a project-owner step with real credentials — an agent should stop at "builds and installs a debug/unsigned-release APK," matching this monorepo's existing pattern for OpenPhotoId/OpenPDFEdit releases.

**iOS.**
- Targets: `aarch64-apple-ios` (device), `aarch64-apple-ios-sim` (Apple Silicon simulator), `x86_64-apple-ios` (Intel simulator, for CI on older runners).
- `cargokit` builds an XCFramework bundling the device+simulator slices and wires it into the Flutter iOS runner's Xcode project automatically.
- `Info.plist` needs `NSCameraUsageDescription` and `NSPhotoLibraryUsageDescription`/`NSPhotoLibraryAddUsageDescription` strings — write these to actually describe local-only scanning, since App Store review reads them.
- Signing/provisioning/App Store submission is a project-owner step with real Apple credentials — an agent should stop at "builds and runs in Simulator," same boundary as Android.

**CI.**
- Fast job on every push: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `cargo test --workspace` — all on the CI host, no mobile toolchain, matching the "testable on desktop/CI" constraint literally.
- Slower job (tag or manual trigger): build the Android AAR and iOS XCFramework via `cargokit`, then `flutter build apk`/`flutter build ios --no-codesign` — mirrors OpenPDFEdit's existing installer-build CI pattern in this monorepo.
- `flutter analyze` + `flutter test` on every push alongside the Rust checks.

## 5. Milestone plan

Each milestone is scoped to ~2–3 weeks for one engineer (assumption #3). Acceptance criteria are the gate to call a milestone done — no milestone is "done" on vibes.

**M0 — Rust core foundation** (no Flutter yet)
- Tasks: workspace scaffold; `docscan-core` image IO; `docscan-detect` quad detection; `docscan-transform` perspective warp; `docscan-filters` (B/W, brightness, enhance); CI (fmt/clippy/deny/test); labeled synthetic-image test dataset with IoU harness.
- Acceptance: `cargo test --workspace` green; detection IoU ≥ 0.9 on the synthetic dataset (§6); all four crates buildable and testable with zero mobile toolchain present.

**M1 — Flutter shell + bridge wiring**
- Tasks: Flutter app scaffold; `flutter_rust_bridge`/`cargokit` integration for Android+iOS; camera capture screen (manual shutter, no processing yet); import screen; permissions flow; raw captured/imported image displayed end-to-end through the bridge.
- Acceptance: a captured or imported photo's raw bytes round-trip through the Rust core and back, displayed on-screen, on a real Android device and iOS Simulator.

**M2 — Detection + interactive correction**
- Tasks: wire `find_document_quad` to the live preview (throttled) and the captured photo; crop screen with draggable corner handles seeded from the detected quad; wire `warp_to_quad` on confirm.
- Acceptance: on a real device, photographing a printed test page yields a corrected, cropped page image the user can visually confirm is properly rectified; manual corner adjustment works and re-warps correctly.

**M3 — Filters, multi-page, export, library**
- Tasks: filter screen (B/W/brightness/enhance) with live preview; page tray for multi-page assembly (reorder/add/delete); `docscan-pdf` export wired end-to-end; `docscan-store` library (save/browse/delete, startup integrity check); `share_plus` wiring.
- Acceptance: capture 3+ pages, apply a filter, export a multi-page PDF, confirm it opens correctly in the OS's native PDF viewer after being shared; force-kill the app mid-flow and confirm no partial/corrupt document appears in the library.

**M4 — Offline OCR (flagged higher R&D risk, may slip independently of other milestones)**
- Tasks: OCR model selection/licensing check; `docscan-ocr` trait + `tract` backend; embed model in app assets (no download); invisible text layer in exported PDFs; FTS5 search wired into the library screen; settings toggle to disable OCR.
- Acceptance: OCR accuracy meets the go/no-go threshold defined in §6 on the fixed test corpus; search-by-text finds a document by content in the library screen; airplane-mode test confirms zero network calls during OCR.

**M5 — Mobile build hardening**
- Tasks: Android release build (AAB, per-ABI splits, stripped symbols) and iOS build validated in Simulator; app icons/branding; performance pass against the budgets in §6; accessibility pass (screen reader labels, contrast); memory-ceiling validation on multi-page export.
- Acceptance: measured perf/memory numbers from §6's success metrics on real (not just emulator) mid-tier hardware; no crashes across the full capture→export→share flow in a 30-minute manual soak.

**M6 — Test hardening + store prep**
- Tasks: fill any test-plan gaps found during M0–M5; write the privacy policy (trivial — no data leaves the device, still required by both stores); store listing assets/metadata; beta distribution setup (TestFlight/Play internal testing).
- Acceptance: full CI suite green; store listing drafts ready for project-owner review; a beta build installable by an external tester with no dev-mode steps.

## 6. Test plan

- **Rust unit tests per crate**: `docscan-detect` against synthetic images with known ground-truth quads; `docscan-transform` round-trip checks (warp then inverse-warp returns the original within tolerance; known-angle skew corrects to within N pixels); `docscan-pdf` page-count assertions and byte-hash stability given fixed inputs and a fixed timestamp/id (the brief's "deterministic test pipeline" requirement — export must not embed wall-clock time or random IDs in a way that breaks reproducible hashing).
- **Labeled image dataset + corner IoU**: a fixture set combining synthetic pages (generated in-test, known corners exactly) and a small set of real photographed test pages (self-captured to avoid licensing issues with third-party images), each with ground-truth corner JSON. Gate: mean IoU ≥ 0.9, with a hard floor per-image (e.g. no single image below 0.75) so one bad outlier can't hide behind a good average.
- **PDF assertions**: exported page count matches input page count; per-page image checksum matches the source (post-filter) image; full-document hash is stable across repeated exports of the same input (deterministic pipeline requirement).
- **OCR accuracy regression** (M4+): character/word error rate against a small fixed ground-truth text corpus; define the go/no-go threshold before starting M4 (e.g. ≥ 90% word accuracy on clean printed text) and track it release-over-release so silent regressions get caught.
- **Flutter widget/integration tests**: capture flow with a mocked camera channel; import flow; crop-screen drag interaction; library CRUD + search; share-intent invocation (mocked platform channel); golden tests for the capture, crop, and library screens.
- **Performance/memory budgets**: detect+warp under a fixed time budget (e.g. < 300ms) for a 12MP photo on mid-tier hardware; PDF export streams pages one at a time rather than holding an entire multi-page document in memory at once (verify via a memory-ceiling test with a 20-page document); binary size budget per §1's success metrics.
- **CI gates**: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `cargo test --workspace` on every push (host-only); `flutter analyze` + `flutter test` on every push; mobile build jobs on tag/manual trigger only (they're slow).

## 7. Risks and mitigations

| Risk | Mitigation |
|---|---|
| **Camera capture complexity** across Android vendors | Use the official `camera` plugin (CameraX-backed) rather than raw platform channels or a custom native camera integration; test on a real low/mid-tier physical device early in M1/M2 — this monorepo's own history (OpenPhotoId, OpenPDFEdit) repeatedly found real-hardware-only bugs that an all-green test suite missed, so budget real-device time explicitly rather than trusting emulator-only testing. |
| **Rust/Flutter bridging** | `flutter_rust_bridge` + `cargokit` is the mature, widely used path for exactly this pairing; keep the FFI surface coarse-grained (whole images/documents, not chatty per-pixel calls) to minimize marshalling bugs. |
| **iOS build/signing** | Stop agent-side work at "builds and runs in Simulator"; signing, provisioning, and App Store submission are project-owner steps with real Apple credentials, documented as a runbook — same boundary already used for OpenPhotoId/OpenPDFEdit releases in this monorepo. |
| **Android binary size** | Ship an Android App Bundle with per-ABI splits (not a universal APK); strip release symbols; keep the OCR model combined size under the budget in §1; measure actual installed size on a release build, not just estimate it. |
| **OCR quality** | The pure-Rust `tract`-based path is real (already proven for mobile in OpenPhotoId) but the specific OCR crate ecosystem is young — treat M4 as an explicit R&D spike with a defined go/no-go accuracy gate (§6) before committing further UI work to it; document a fallback of Tesseract-via-FFI (mature, accurate, but reintroduces a native C dependency and more complex mobile cross-compilation) if the pure-Rust path can't clear the accuracy bar in a reasonable timebox. |
| **App store compliance** | No accounts and no data leaving the device makes the privacy policy simple, but both stores still require one — write it plainly and truthfully; get permission usage strings exactly right; enforce a hard project rule of zero telemetry/analytics/crash-SDK dependencies so the "100% local" claim is never accidentally false. |
| **OCR/model licensing** | Verify the specific exported ONNX weights' license before bundling (PaddleOCR upstream is Apache-2.0) — apply the same non-commercial-weights ban this monorepo's OpenPhotoId app already established for its own vision models. |

## 8. First implementation checklist (M0 kickoff)

This is the only section written at bite-sized/TDD granularity — it's the concrete first slice to execute now. Everything from M1 onward needs its own dedicated plan drafted at kickoff, since it spans genuinely independent subsystems (bridge, two mobile platforms, UI) that shouldn't be pre-committed to task-level detail before M0's actual API surface exists.

- [ ] **Step 1: Scaffold the Cargo workspace**

Create `opendocscan/Cargo.toml` — the root of *this app's* Cargo workspace, which sits in a subdirectory of the openapps monorepo alongside the other apps, not at the monorepo root:

```toml
[workspace]
resolver = "2"
members = [
    "crates/docscan-core",
    "crates/docscan-detect",
]

[workspace.package]
edition = "2021"
license = "MIT OR Apache-2.0"
```

- [ ] **Step 2: Create `docscan-core` with a round-trip image IO test**

`crates/docscan-core/Cargo.toml`:

```toml
[package]
name = "docscan-core"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
image = "0.25"
thiserror = "2"
```

`crates/docscan-core/src/lib.rs`:

```rust
use image::DynamicImage;
use std::path::Path;

#[derive(thiserror::Error, Debug)]
pub enum CoreError {
    #[error("image decode/encode failed: {0}")]
    Image(#[from] image::ImageError),
}

pub fn load_image(path: &Path) -> Result<DynamicImage, CoreError> {
    Ok(image::open(path)?)
}

pub fn save_image(img: &DynamicImage, path: &Path) -> Result<(), CoreError> {
    img.save(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_png_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");

        let original = DynamicImage::new_rgb8(4, 4);
        save_image(&original, &path).unwrap();
        let loaded = load_image(&path).unwrap();

        assert_eq!(original.dimensions(), loaded.dimensions());
    }
}
```

Add `tempfile = "3"` under `[dev-dependencies]`.

- [ ] **Step 3: Run it, confirm it fails, then passes**

Run: `cargo test -p docscan-core`
Expected first run (before the crate exists): fails to compile — that's the "red" before writing the files above. Once `lib.rs` is in place: `test round_trips_a_png_through_disk ... ok`.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml crates/docscan-core
git commit -m "core: scaffold workspace and image IO round-trip"
```

- [ ] **Step 5: Create `docscan-detect` with a failing quad-detection test**

`crates/docscan-detect/Cargo.toml`:

```toml
[package]
name = "docscan-detect"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
image = "0.25"
imageproc = "0.25"

[dev-dependencies]
```

`crates/docscan-detect/src/lib.rs` — write the test first:

```rust
pub type Point = (f32, f32);

pub fn find_document_quad(_img: &image::DynamicImage) -> Option<[Point; 4]> {
    None // not implemented yet — this is what makes the test below fail
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};
    use imageproc::drawing::draw_polygon_mut;
    use imageproc::point::Point as IPoint;

    fn synthetic_page(width: u32, height: u32, quad: [Point; 4]) -> image::DynamicImage {
        let mut buf = ImageBuffer::from_pixel(width, height, Rgb([20u8, 20, 20]));
        let poly: Vec<IPoint<i32>> = quad
            .iter()
            .map(|&(x, y)| IPoint::new(x as i32, y as i32))
            .collect();
        draw_polygon_mut(&mut buf, &poly, Rgb([230, 230, 230]));
        image::DynamicImage::ImageRgb8(buf)
    }

    fn rasterize_quad(quad: &[Point; 4], width: u32, height: u32) -> Vec<bool> {
        let mut buf: ImageBuffer<image::Luma<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(width, height, image::Luma([0]));
        let poly: Vec<IPoint<i32>> = quad
            .iter()
            .map(|&(x, y)| IPoint::new(x as i32, y as i32))
            .collect();
        imageproc::drawing::draw_polygon_mut(&mut buf, &poly, image::Luma([255]));
        buf.pixels().map(|p| p.0[0] > 0).collect()
    }

    fn quad_iou(a: &[Point; 4], b: &[Point; 4], width: u32, height: u32) -> f64 {
        let mask_a = rasterize_quad(a, width, height);
        let mask_b = rasterize_quad(b, width, height);
        let intersection = mask_a.iter().zip(&mask_b).filter(|(x, y)| **x && **y).count();
        let union = mask_a.iter().zip(&mask_b).filter(|(x, y)| **x || **y).count();
        intersection as f64 / union.max(1) as f64
    }

    #[test]
    fn detects_a_synthetic_page_within_iou_threshold() {
        let expected: [Point; 4] = [(40.0, 30.0), (360.0, 45.0), (350.0, 270.0), (30.0, 260.0)];
        let img = synthetic_page(400, 300, expected);

        let detected = find_document_quad(&img).expect("should detect a quad");
        let iou = quad_iou(&expected, &detected, 400, 300);

        assert!(iou > 0.9, "IoU {iou} below the 0.9 threshold");
    }
}
```

- [ ] **Step 6: Run it, confirm it fails**

Run: `cargo test -p docscan-detect`
Expected: FAIL — `find_document_quad` panics the `.expect(...)` since it always returns `None`.

- [ ] **Step 7: Implement the minimal detector**

Replace the stub in `find_document_quad` with a real (if simple, refinable-later) implementation: grayscale → Canny edges → contours → take the largest-area contour → reduce it to its four extreme points as the quad approximation.

```rust
use image::{DynamicImage, GrayImage};
use imageproc::contours::find_contours;
use imageproc::edges::canny;

pub type Point = (f32, f32);

pub fn find_document_quad(img: &DynamicImage) -> Option<[Point; 4]> {
    let gray: GrayImage = img.to_luma8();
    let edges = canny(&gray, 50.0, 100.0);
    let contours = find_contours::<i32>(&edges);

    contours
        .into_iter()
        .filter(|c| c.points.len() >= 4)
        .map(|c| extreme_quad(&c.points))
        .max_by(|a, b| polygon_area(a).total_cmp(&polygon_area(b)))
}

fn extreme_quad(points: &[imageproc::point::Point<i32>]) -> [Point; 4] {
    let (mut top, mut bottom, mut left, mut right) = (points[0], points[0], points[0], points[0]);
    for &p in points {
        if p.y < top.y { top = p; }
        if p.y > bottom.y { bottom = p; }
        if p.x < left.x { left = p; }
        if p.x > right.x { right = p; }
    }
    [
        (top.x as f32, top.y as f32),
        (right.x as f32, right.y as f32),
        (bottom.x as f32, bottom.y as f32),
        (left.x as f32, left.y as f32),
    ]
}

fn polygon_area(quad: &[Point; 4]) -> f64 {
    let mut area = 0.0_f64;
    for i in 0..4 {
        let (x1, y1) = quad[i];
        let (x2, y2) = quad[(i + 1) % 4];
        area += (x1 as f64) * (y2 as f64) - (x2 as f64) * (y1 as f64);
    }
    (area / 2.0).abs()
}
```

*Note: verify exact `imageproc`/`image` API signatures (this plan targets `imageproc 0.25`) against whatever version is actually pinned at implementation time — point releases do shift function signatures in this crate.*

- [ ] **Step 8: Run it, confirm it passes**

Run: `cargo test -p docscan-detect`
Expected: `test detects_a_synthetic_page_within_iou_threshold ... ok`

- [ ] **Step 9: Wire up CI**

GitHub Actions only reads workflow files from the **monorepo root**'s `.github/workflows/` — a `ci.yml` under `opendocscan/` is never executed. So add an `opendocscan-rust` job to the existing root workflow (`.github/workflows/ci.yml`), following the `openpdfedit-rust` job already there: scope it with `defaults.run.working-directory: opendocscan` (without it the commands run against the *root* Cargo workspace, not this one) and key the cache with `workspaces: opendocscan -> target`.

```yaml
  opendocscan-rust:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: opendocscan
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: opendocscan -> target
      - name: Format
        run: cargo fmt --all --check
      - name: Clippy
        run: cargo clippy --workspace --all-targets -- -D warnings
      - name: Test
        run: cargo test --workspace
```

(Add `cargo-deny` once a `deny.toml` license policy is copied over from a sibling project in this monorepo — OpenPDFEdit/OpenPhotoId both already have one to use as a template; `openpdfedit-license-policy` in the same root workflow is the job to copy.)

- [ ] **Step 10: Commit**

```bash
git add opendocscan/crates/docscan-detect .github/workflows/ci.yml
git commit -m "detect: implement synthetic-quad detection with IoU test + CI"
```

---

**Next step after this checklist**: M1's bite-sized plan is now written — see `docs/superpowers/plans/2026-08-05-opendocscan-m1-flutter-shell-bridge.md` (Flutter shell, `flutter_rust_bridge`/`cargokit` wiring, camera capture + import screens, end-to-end bridge round-trip, real-device verification checklist). Execute it once M0 above is green. M2 onward still need their own dedicated plans drafted at kickoff, once each prior milestone's real API surface exists.

---

## 12. Web app (added 2026-08-26)

### Reversing §8's form judgment

Section 8 concluded that browser and web forms were a blank in this market
for a reason rather than as an opportunity: *"文档扫描依赖手机摄像头这一物理能力，
浏览器插件/Web站运行在电脑端，无法调用手机摄像头完成核心拍摄动作"* — that scanning
needs a phone camera and browser apps run on desktops.

The premise is out of date. `getUserMedia({ facingMode: "environment" })`
opens the rear camera on every current mobile browser, and everything
downstream of the frame is arithmetic on a pixel buffer. The M0 core
compiles to `wasm32-unknown-unknown` with no changes and no conditional
code, at 318KB — 114KB over the wire.

So §8's conclusion is superseded for the web target specifically. Its
reasoning about *desktop* apps still holds and is untouched: nobody wants
to scan a document by holding it up to a laptop.

Three things the web form gives that the app form cannot:

1. **No install and no review queue.** §9's切入点1 is a timed window —
   Microsoft Lens stops scanning in March 2026 and its users are looking
   for a replacement now. A URL reaches them this week.
2. **The privacy claim becomes falsifiable.** §9's切入点2 rests on a
   promise a user must take on trust in a native app. In a tab they can
   open DevTools and check. That converts the central marketing claim into
   something a sceptic can verify in thirty seconds — and per §9, a
   reputation for "说到做到" is the part of that moat that is actually
   defensible.
3. **It de-risks the mobile build.** The whole pipeline gets exercised on
   real hardware, in front of real users, before any of M1's
   `flutter_rust_bridge`, cargokit, NDK, or XCFramework work begins.

This does not replace M1–M6. The mobile apps remain the primary form, per
§8's finding that 9 of 10 competitors are apps and that store search is
this category's real acquisition channel.

### What shipped

- `docscan-ocr` — the `OcrEngine` trait and word-box geometry. No model:
  it owns the shape of a recognition result so that a `tract` backend on
  mobile and a WASM one in the browser are two implementations of one
  trait rather than two pipelines.
- `docscan-pdf` — multi-page assembly, deterministic byte output,
  invisible OCR text layer, bilevel pages packed a bit per pixel.
- `docscan-wasm` — the browser bridge. Adds only what a tab needs and the
  core should not know: a detection working size that keeps up with a live
  preview, a page ceiling that keeps a mobile tab alive, and a streaming
  PDF builder.
- `apps/web` — the PWA. Camera, import, corner editor, filters, page
  tray, IndexedDB library with full-text search, PDF export, offline
  service worker, vendored OCR engine.

### Performance

Measured, then fixed, then measured again; `apps/web/BENCHMARKS.md` holds
the numbers and the method. The parts that change the plan rather than
just the web app:

- **`docscan-filters` is now a slice API with `DynamicImage` wrappers
  over it.** Contrast stretching and brightness are both per-channel maps,
  so they compose into one 256-entry lookup table applied in one in-place
  pass. `enhance` was sorting a copy of every pixel to read two
  percentiles; a histogram answers the same question in one pass and a
  kilobyte. 20x, and it applies to the mobile build too.
- **`docscan-transform` grew `warp_rgba_to_quad`,** a hand-written
  projective warp that steps the source coordinate incrementally along
  each output row. It exists to avoid two full-image colour conversions on
  every capture, and is *also* more accurate than the `imageproc` path:
  that one truncates to `u8` three times per pixel, because its
  `Clamp<f32> for u8` is a bare `as u8`. A cross-check test pins the two
  together and asserts the difference is the upward rounding correction
  rather than a sampling error.
- **The wasm entry points take `Vec<u8>` rather than `&[u8]`.**
  `wasm_bindgen` copies the caller's array into our heap either way; the
  slice version then copied it again. 48MB a capture, not 96MB.
- **`profile.wasm-release` is `opt-level = 3`, not `"z"`.** This was the
  wrong trade and it cost 7x on detection to save nine kilobytes of
  transfer. §4 should say so: for a module that is cached after the first
  visit, size is the cheaper axis.
- **WebAssembly SIMD buys nothing here** and is off by default. The loops
  look vectorisable and are not — a lookup table indexed by each byte, and
  a warp gathering four scattered pixels — so both are indirection.
- **All pixel work runs in a Web Worker.** One complete scan went from
  five long tasks totalling 796ms of frozen interface to zero.

### Deltas against this plan's assumptions

- **Assumption 5** ("OCR default: on by default per document, bundled
  model, no download") holds on mobile but is relaxed on the web: the
  engine is ~6MB and is fetched from this app's own origin on first use,
  then cached. Bundling it in the initial load would put 6MB in front of a
  user who has not yet scanned anything.
- **§3's pure-Rust OCR path** (`tract` + PaddleOCR ONNX) remains the
  mobile plan and remains M4's R&D risk. The web app uses a locally
  vendored tesseract.js behind the same `OcrEngine`-shaped seam, which
  means M4 can be evaluated against a working baseline rather than
  against nothing.
- **§6's "PDF export streams pages one at a time"** is implemented as a
  builder rather than a function over a page array, on both sides.

### Still open

- No `cargo deny` / `cargo fmt --check` / clippy CI job yet (§4 asks for
  one).
- The invisible text layer is Latin-script only; see `docscan-pdf`'s
  `encode_winansi`.
