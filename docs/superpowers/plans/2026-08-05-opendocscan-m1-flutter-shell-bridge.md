# OpenDocScan M1 — Flutter Shell + Bridge Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the Flutter app shell, wire `flutter_rust_bridge` + `cargokit` to the existing Rust workspace, and get a captured-or-imported photo's raw bytes round-tripping through the Rust core and back onto the screen, on a real Android device and iOS Simulator.

**Architecture:** A new `app/` Flutter project (standard `flutter create` layout) gains a `docscan-ffi` Rust crate as a new workspace member, exposing a minimal `flutter_rust_bridge`-annotated API. Camera capture and file import are each wrapped in a small Dart interface (`CaptureController`, an injected `FilePickerPlatform`) so the UI is unit-testable on host without a real device, while a real-device manual pass (Task 9) covers what can't be simulated.

**Tech stack:** Flutter 3.x/Dart; `flutter_rust_bridge` v2 + `cargokit`; `camera`, `file_picker`, `permission_handler` Flutter packages; existing `docscan-core` Rust crate from M0.

## Global Constraints

Carried forward from `opendocscan/PLAN.md` (apply to every task below):

- 100% local processing — no network calls anywhere in this flow.
- Rust core stays platform-agnostic and testable without a mobile toolchain; only `docscan-ffi` is mobile-specific glue.
- Flutter layer handles camera, file picker, permissions, previews only — no image processing logic in Dart.
- No telemetry/analytics/crash-reporting SDK.
- Android minSdk 24, iOS 15+ (per PLAN.md assumption #6).

**Prerequisite:** M0 (`opendocscan/PLAN.md` §8) is complete — `crates/docscan-core` and `crates/docscan-detect` exist, build, and pass `cargo test --workspace`. This plan assumes that workspace `Cargo.toml` already exists at `opendocscan/Cargo.toml` — the root of this app's own Cargo workspace, a subdirectory of the openapps monorepo, not the monorepo root — with those two members.

---

## File Structure

New/modified files this plan produces:

- `crates/docscan-core/src/lib.rs` — **modify**: add `load_image_from_bytes` (M0 only had path-based IO; the bridge passes bytes, not paths).
- `crates/docscan-ffi/Cargo.toml`, `crates/docscan-ffi/src/api.rs` — **create**: the `flutter_rust_bridge`-annotated surface (`ping`, `decode_dimensions`).
- `Cargo.toml` (workspace root) — **modify**: add `crates/docscan-ffi` to `members`.
- `app/pubspec.yaml`, `app/lib/main.dart` — **create**: Flutter app scaffold.
- `app/lib/bridge/` — **generated** by `flutter_rust_bridge_codegen`; never hand-edited.
- `app/rust_builder/` — **generated** by `cargokit`'s Flutter-package init; never hand-edited.
- `app/lib/permissions/permission_gate.dart` — **create**: injectable permission-check interface.
- `app/lib/capture/capture_controller.dart`, `app/lib/capture/capture_screen.dart` — **create**: camera capture screen behind a fake-able controller interface.
- `app/lib/import/import_screen.dart` — **create**: file import screen using `file_picker`.
- `app/lib/home_screen.dart` — **create**: entry screen wiring capture/import together and calling the bridge.
- Test files mirror each of the above under `app/test/`.

**Interfaces summary** (full detail repeated per-task below, so a task's implementer doesn't need to read neighboring tasks):
- `docscan_core::load_image_from_bytes(bytes: &[u8]) -> Result<DynamicImage, CoreError>` (Task 1)
- `docscan_ffi::api::{ping() -> String, decode_dimensions(bytes: Vec<u8>) -> DecodedInfo}`, `DecodedInfo { width: u32, height: u32 }` (Task 3)
- `PermissionGate` abstract interface + `PermissionGate.request(Permission) -> Future<bool>` (Task 5)
- `CaptureController` abstract interface + `CaptureController.capture() -> Future<Uint8List>` (Task 6)
- `ImportScreen.onFilesSelected: void Function(List<Uint8List>)` callback (Task 7)

---

### Task 1: Byte-based image loading in `docscan-core`

**Files:**
- Modify: `crates/docscan-core/src/lib.rs`
- Test: same file, `#[cfg(test)] mod tests`

**Interfaces:**
- Produces: `pub fn load_image_from_bytes(bytes: &[u8]) -> Result<DynamicImage, CoreError>` — consumed by Task 3's `decode_dimensions`.

- [ ] **Step 1: Write the failing test**

Add to `crates/docscan-core/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // existing round_trips_a_png_through_disk test stays above this one

    #[test]
    fn loads_an_image_from_in_memory_bytes() {
        let original = DynamicImage::new_rgb8(4, 4);
        let mut bytes: Vec<u8> = Vec::new();
        original
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();

        let loaded = load_image_from_bytes(&bytes).unwrap();

        assert_eq!(original.dimensions(), loaded.dimensions());
    }
}
```

- [ ] **Step 2: Run it, confirm it fails**

Run: `cargo test -p docscan-core loads_an_image_from_in_memory_bytes`
Expected: FAIL — `load_image_from_bytes` doesn't exist yet (compile error).

- [ ] **Step 3: Implement it**

Add above the test module in `crates/docscan-core/src/lib.rs`:

```rust
pub fn load_image_from_bytes(bytes: &[u8]) -> Result<DynamicImage, CoreError> {
    Ok(image::load_from_memory(bytes)?)
}
```

- [ ] **Step 4: Run it, confirm it passes**

Run: `cargo test -p docscan-core`
Expected: both `round_trips_a_png_through_disk` and `loads_an_image_from_in_memory_bytes` pass.

- [ ] **Step 5: Commit**

```bash
git add crates/docscan-core/src/lib.rs
git commit -m "core: load images from in-memory bytes, not just disk paths"
```

---

### Task 2: Flutter app scaffold

**Files:**
- Create: `app/` (via `flutter create`)
- Modify: `app/lib/main.dart`
- Create: `app/test/main_test.dart`

**Interfaces:**
- Produces: a running `MaterialApp` shell that Task 4 onward add screens/routes to. No other task depends on internals beyond "there is a `main.dart` with a `MyApp` widget."

- [ ] **Step 1: Scaffold the project**

Run from `opendocscan/`:

```bash
flutter create --org com.opendocscan --project-name opendocscan_app app
```

- [ ] **Step 2: Write the failing test**

`app/test/main_test.dart`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:opendocscan_app/main.dart';

void main() {
  testWidgets('app shows the OpenDocScan title', (tester) async {
    await tester.pumpWidget(const MyApp());
    expect(find.text('OpenDocScan'), findsOneWidget);
  });
}
```

- [ ] **Step 2: Run it, confirm it fails**

Run: `cd app && flutter test test/main_test.dart`
Expected: FAIL — the default `flutter create` counter-demo app doesn't show "OpenDocScan" anywhere.

- [ ] **Step 3: Replace the generated demo with a minimal shell**

`app/lib/main.dart`:

```dart
import 'package:flutter/material.dart';

void main() {
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'OpenDocScan',
      home: Scaffold(
        appBar: AppBar(title: const Text('OpenDocScan')),
        body: const Center(child: Text('OpenDocScan')),
      ),
    );
  }
}
```

- [ ] **Step 4: Run it, confirm it passes**

Run: `cd app && flutter test test/main_test.dart`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add app
git commit -m "app: scaffold Flutter shell"
```

---

### Task 3: `docscan-ffi` crate with a `ping` + `decode_dimensions` API

**Files:**
- Create: `crates/docscan-ffi/Cargo.toml`
- Create: `crates/docscan-ffi/src/api.rs`
- Modify: `Cargo.toml` (workspace root) — add `crates/docscan-ffi` to `members`

**Interfaces:**
- Consumes: `docscan_core::load_image_from_bytes` (Task 1).
- Produces: `pub fn ping() -> String`, `pub fn decode_dimensions(bytes: Vec<u8>) -> DecodedInfo`, `pub struct DecodedInfo { pub width: u32, pub height: u32 }` — consumed by Task 4 (codegen) and Task 8 (end-to-end wiring).

This task is pure Rust — no `flutter_rust_bridge` codegen or Flutter involvement yet, so it's tested with plain `cargo test`.

- [ ] **Step 1: Add the crate to the workspace**

`Cargo.toml` (root):

```toml
[workspace]
resolver = "2"
members = [
    "crates/docscan-core",
    "crates/docscan-detect",
    "crates/docscan-ffi",
]
```

`crates/docscan-ffi/Cargo.toml`:

```toml
[package]
name = "docscan-ffi"
version = "0.1.0"
edition.workspace = true
license.workspace = true

[dependencies]
docscan-core = { path = "../docscan-core" }
flutter_rust_bridge = "2"

[lib]
crate-type = ["cdylib", "staticlib", "lib"]
```

- [ ] **Step 2: Write the failing test**

`crates/docscan-ffi/src/api.rs`:

```rust
pub struct DecodedInfo {
    pub width: u32,
    pub height: u32,
}

pub fn ping() -> String {
    todo!()
}

pub fn decode_dimensions(bytes: Vec<u8>) -> DecodedInfo {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_returns_a_fixed_string() {
        assert_eq!(ping(), "docscan-ffi-ok");
    }

    #[test]
    fn decode_dimensions_reports_a_pngs_size() {
        let img = image::DynamicImage::new_rgb8(8, 6);
        let mut bytes: Vec<u8> = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();

        let info = decode_dimensions(bytes);

        assert_eq!(info.width, 8);
        assert_eq!(info.height, 6);
    }
}
```

Add `image = "0.25"` under `[dev-dependencies]` in `crates/docscan-ffi/Cargo.toml` (only the test module uses it directly; production code goes through `docscan-core`).

- [ ] **Step 3: Run it, confirm it fails**

Run: `cargo test -p docscan-ffi`
Expected: FAIL — both `todo!()` bodies panic.

- [ ] **Step 4: Implement it**

Replace the two stub functions:

```rust
pub fn ping() -> String {
    "docscan-ffi-ok".to_string()
}

pub fn decode_dimensions(bytes: Vec<u8>) -> DecodedInfo {
    let img = docscan_core::load_image_from_bytes(&bytes)
        .expect("bridge caller is responsible for passing decodable image bytes");
    DecodedInfo {
        width: img.width(),
        height: img.height(),
    }
}
```

*Note: a real bridge API should return a `Result` so decode failures surface to Dart as catchable errors rather than panicking across the FFI boundary — deferred here since this task only proves the plumbing; tighten this before M2 starts relying on user-supplied (as opposed to test) images.*

- [ ] **Step 5: Run it, confirm it passes**

Run: `cargo test -p docscan-ffi`
Expected: both tests pass.

- [ ] **Step 6: Add the crate's lib.rs**

`crates/docscan-ffi/src/lib.rs`:

```rust
pub mod api;
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/docscan-ffi
git commit -m "ffi: add docscan-ffi crate with ping + decode_dimensions"
```

---

### Task 4: Wire `flutter_rust_bridge` + `cargokit`, call `ping()` from Dart

**Files:**
- Create: `app/rust_builder/` (generated by cargokit's `flutter_rust_bridge_codegen integrate` step)
- Create: `app/lib/bridge/` (generated Dart bindings)
- Modify: `app/pubspec.yaml`, `app/lib/main.dart`
- Test: `app/test/bridge_test.dart`

**Interfaces:**
- Consumes: `docscan_ffi::api::ping` (Task 3).
- Produces: a generated `RustLib` (or equivalent, per the version of `flutter_rust_bridge_codegen` actually installed — confirm the exact generated entrypoint name against its current quickstart docs) that Task 8 also calls for `decode_dimensions`.

- [ ] **Step 1: Install the codegen tool and run integration**

```bash
cargo install flutter_rust_bridge_codegen
cd app
flutter_rust_bridge_codegen integrate --rust-crate-dir ../crates/docscan-ffi
```

This generates `app/rust_builder/` (the cargokit build glue) and wires `app/android/`/`app/ios/` to build `docscan-ffi` as part of `flutter build`/`flutter run`, and adds `flutter_rust_bridge` to `app/pubspec.yaml`.

*Note: the exact CLI flags/subcommand shape (`integrate` vs. separate `generate`+manual cargokit wiring) has shifted across `flutter_rust_bridge_codegen` versions — verify against whatever version `cargo install` actually resolves at implementation time and adjust this step's command, keeping the deliverable (generated bindings + cargokit wiring present) the same.*

- [ ] **Step 2: Generate the Dart bindings for the current API**

```bash
cd app
flutter_rust_bridge_codegen generate
```

This reads `crates/docscan-ffi/src/api.rs` and emits `app/lib/bridge/` (or the configured output path) containing a Dart function corresponding to `ping()`.

- [ ] **Step 3: Write the failing test**

`app/test/bridge_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:opendocscan_app/bridge/frb_generated.dart';

void main() {
  setUpAll(() async {
    await RustLib.init();
  });

  test('ping returns the fixed string from Rust', () async {
    expect(await ping(), 'docscan-ffi-ok');
  });
}
```

*Note: `RustLib.init()` and the generated function's exact import path/name depend on the `flutter_rust_bridge` version's codegen output — after running Step 2, check the actual generated file for the real entrypoint name and adjust this import if it differs from `frb_generated.dart`/`RustLib`.*

- [ ] **Step 4: Run it, confirm it fails**

Run: `cd app && flutter test test/bridge_test.dart`
Expected: FAIL until the native library for the host platform is built (see Step 5) — this is a real dependency, not a design flaw: this test loads an actual compiled `.dylib`/`.so`, unlike a pure-Dart widget test.

- [ ] **Step 5: Build the host-native library and re-run**

```bash
cd crates/docscan-ffi && cargo build --release
cd ../../app && flutter test test/bridge_test.dart
```

Expected: PASS — confirms the codegen'd Dart binding actually calls into the real Rust `ping()`, not a mock.

- [ ] **Step 6: Display it on screen**

`app/lib/main.dart` — add a widget that calls `ping()` and shows the result, so the app itself (not just the test) proves the wiring:

```dart
import 'package:flutter/material.dart';
import 'bridge/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'OpenDocScan',
      home: Scaffold(
        appBar: AppBar(title: const Text('OpenDocScan')),
        body: Center(
          child: FutureBuilder<String>(
            future: ping(),
            builder: (context, snapshot) =>
                Text(snapshot.data ?? 'loading bridge...'),
          ),
        ),
      ),
    );
  }
}
```

Update `app/test/main_test.dart`'s expectation accordingly if the literal "OpenDocScan" text moved — keep at least one widget test asserting the `AppBar` title so Task 2's test doesn't silently break.

- [ ] **Step 7: Run the full Flutter test suite**

Run: `cd app && flutter test`
Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
git add app crates/docscan-ffi
git commit -m "app: wire flutter_rust_bridge + cargokit, prove ping() round-trips"
```

---

### Task 5: Permission gate abstraction

**Files:**
- Create: `app/lib/permissions/permission_gate.dart`
- Test: `app/test/permissions/permission_gate_test.dart`
- Modify: `app/pubspec.yaml` (add `permission_handler`)

**Interfaces:**
- Produces: `abstract class PermissionGate { Future<bool> request(AppPermission permission); }`, `enum AppPermission { camera, photos }`, and a real `PermissionHandlerGate implements PermissionGate` — consumed by Task 6 (capture screen) and Task 7 (import screen), which each take a `PermissionGate` so tests can inject a fake instead of touching the real OS permission system.

- [ ] **Step 1: Add the dependency**

`app/pubspec.yaml`, under `dependencies:`:

```yaml
  permission_handler: ^11.0.0
```

Run: `cd app && flutter pub get`

- [ ] **Step 2: Write the failing test**

`app/test/permissions/permission_gate_test.dart`:

```dart
import 'package:flutter_test/flutter_test.dart';
import 'package:opendocscan_app/permissions/permission_gate.dart';

class FakePermissionGate implements PermissionGate {
  final Map<AppPermission, bool> grants;
  FakePermissionGate(this.grants);

  @override
  Future<bool> request(AppPermission permission) async =>
      grants[permission] ?? false;
}

void main() {
  test('fake gate reports granted/denied per permission', () async {
    final gate = FakePermissionGate({AppPermission.camera: true});

    expect(await gate.request(AppPermission.camera), isTrue);
    expect(await gate.request(AppPermission.photos), isFalse);
  });
}
```

- [ ] **Step 3: Run it, confirm it fails**

Run: `cd app && flutter test test/permissions/permission_gate_test.dart`
Expected: FAIL — `PermissionGate`/`AppPermission` don't exist yet.

- [ ] **Step 4: Implement it**

`app/lib/permissions/permission_gate.dart`:

```dart
import 'package:permission_handler/permission_handler.dart' as ph;

enum AppPermission { camera, photos }

abstract class PermissionGate {
  Future<bool> request(AppPermission permission);
}

class PermissionHandlerGate implements PermissionGate {
  @override
  Future<bool> request(AppPermission permission) async {
    final target = switch (permission) {
      AppPermission.camera => ph.Permission.camera,
      AppPermission.photos => ph.Permission.photos,
    };
    final status = await target.request();
    return status.isGranted;
  }
}
```

- [ ] **Step 5: Run it, confirm it passes**

Run: `cd app && flutter test test/permissions/permission_gate_test.dart`
Expected: PASS (the fake, not `PermissionHandlerGate`, is what's exercised — `PermissionHandlerGate` itself needs a real platform, verified manually in Task 9).

- [ ] **Step 6: Commit**

```bash
git add app/pubspec.yaml app/pubspec.lock app/lib/permissions app/test/permissions
git commit -m "app: injectable permission gate abstraction"
```

---

### Task 6: Capture screen behind a fake-able controller

**Files:**
- Create: `app/lib/capture/capture_controller.dart`, `app/lib/capture/capture_screen.dart`
- Test: `app/test/capture/capture_screen_test.dart`
- Modify: `app/pubspec.yaml` (add `camera`)

**Interfaces:**
- Consumes: `PermissionGate` (Task 5).
- Produces: `abstract class CaptureController { Future<void> initialize(); Future<Uint8List> capture(); void dispose(); }`, `class CaptureScreen extends StatefulWidget { const CaptureScreen({required this.permissionGate, required this.controllerFactory, required this.onCaptured}); }` — consumed by Task 8 (which supplies the real `onCaptured` callback wired to the bridge).

- [ ] **Step 1: Add the dependency**

`app/pubspec.yaml`:

```yaml
  camera: ^0.11.0
```

Run: `cd app && flutter pub get`

- [ ] **Step 2: Write the failing test**

`app/test/capture/capture_screen_test.dart`:

```dart
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:opendocscan_app/capture/capture_controller.dart';
import 'package:opendocscan_app/capture/capture_screen.dart';
import 'package:opendocscan_app/permissions/permission_gate.dart';

class FakePermissionGate implements PermissionGate {
  @override
  Future<bool> request(AppPermission permission) async => true;
}

class FakeCaptureController implements CaptureController {
  @override
  Future<void> initialize() async {}

  @override
  Future<Uint8List> capture() async => Uint8List.fromList([1, 2, 3]);

  @override
  void dispose() {}
}

void main() {
  testWidgets('shutter tap invokes onCaptured with the captured bytes',
      (tester) async {
    Uint8List? captured;

    await tester.pumpWidget(MaterialApp(
      home: CaptureScreen(
        permissionGate: FakePermissionGate(),
        controllerFactory: () => FakeCaptureController(),
        onCaptured: (bytes) => captured = bytes,
      ),
    ));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('shutter_button')));
    await tester.pumpAndSettle();

    expect(captured, Uint8List.fromList([1, 2, 3]));
  });
}
```

- [ ] **Step 3: Run it, confirm it fails**

Run: `cd app && flutter test test/capture/capture_screen_test.dart`
Expected: FAIL — none of `CaptureController`/`CaptureScreen` exist yet.

- [ ] **Step 4: Implement the controller interface + real camera-backed implementation**

`app/lib/capture/capture_controller.dart`:

```dart
import 'dart:typed_data';
import 'package:camera/camera.dart';

abstract class CaptureController {
  Future<void> initialize();
  Future<Uint8List> capture();
  void dispose();
}

class CameraCaptureController implements CaptureController {
  CameraController? _controller;

  @override
  Future<void> initialize() async {
    final cameras = await availableCameras();
    _controller = CameraController(cameras.first, ResolutionPreset.high);
    await _controller!.initialize();
  }

  @override
  Future<Uint8List> capture() async {
    final file = await _controller!.takePicture();
    return file.readAsBytes();
  }

  @override
  void dispose() {
    _controller?.dispose();
  }
}
```

- [ ] **Step 5: Implement the screen**

`app/lib/capture/capture_screen.dart`:

```dart
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'capture_controller.dart';
import '../permissions/permission_gate.dart';

class CaptureScreen extends StatefulWidget {
  final PermissionGate permissionGate;
  final CaptureController Function() controllerFactory;
  final void Function(Uint8List bytes) onCaptured;

  const CaptureScreen({
    super.key,
    required this.permissionGate,
    required this.controllerFactory,
    required this.onCaptured,
  });

  @override
  State<CaptureScreen> createState() => _CaptureScreenState();
}

class _CaptureScreenState extends State<CaptureScreen> {
  late final CaptureController _controller;
  bool _ready = false;

  @override
  void initState() {
    super.initState();
    _controller = widget.controllerFactory();
    _setUp();
  }

  Future<void> _setUp() async {
    final granted = await widget.permissionGate.request(AppPermission.camera);
    if (!granted) return;
    await _controller.initialize();
    if (mounted) setState(() => _ready = true);
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!_ready) {
      return const Center(child: CircularProgressIndicator());
    }
    return Center(
      child: ElevatedButton(
        key: const Key('shutter_button'),
        onPressed: () async {
          final bytes = await _controller.capture();
          widget.onCaptured(bytes);
        },
        child: const Text('Capture'),
      ),
    );
  }
}
```

- [ ] **Step 6: Run it, confirm it passes**

Run: `cd app && flutter test test/capture/capture_screen_test.dart`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add app/pubspec.yaml app/pubspec.lock app/lib/capture app/test/capture
git commit -m "app: capture screen behind a fake-able CaptureController"
```

---

### Task 7: Import screen (file_picker)

**Files:**
- Create: `app/lib/import/import_screen.dart`
- Test: `app/test/import/import_screen_test.dart`
- Modify: `app/pubspec.yaml` (add `file_picker`)

**Interfaces:**
- Produces: `class ImportScreen extends StatelessWidget { const ImportScreen({required this.pickerFactory, required this.onFilesSelected}); }` where `pickerFactory: Future<List<Uint8List>> Function()` — consumed by Task 8.

- [ ] **Step 1: Add the dependency**

`app/pubspec.yaml`:

```yaml
  file_picker: ^8.0.0
```

Run: `cd app && flutter pub get`

- [ ] **Step 2: Write the failing test**

`app/test/import/import_screen_test.dart`:

```dart
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:opendocscan_app/import/import_screen.dart';

void main() {
  testWidgets('tapping import invokes onFilesSelected with picked bytes',
      (tester) async {
    List<Uint8List>? picked;

    await tester.pumpWidget(MaterialApp(
      home: ImportScreen(
        pickerFactory: () async => [Uint8List.fromList([9, 9])],
        onFilesSelected: (files) => picked = files,
      ),
    ));

    await tester.tap(find.byKey(const Key('import_button')));
    await tester.pumpAndSettle();

    expect(picked, [Uint8List.fromList([9, 9])]);
  });
}
```

- [ ] **Step 3: Run it, confirm it fails**

Run: `cd app && flutter test test/import/import_screen_test.dart`
Expected: FAIL — `ImportScreen` doesn't exist yet.

- [ ] **Step 4: Implement it**

`app/lib/import/import_screen.dart`:

```dart
import 'dart:typed_data';
import 'package:flutter/material.dart';

class ImportScreen extends StatelessWidget {
  final Future<List<Uint8List>> Function() pickerFactory;
  final void Function(List<Uint8List> files) onFilesSelected;

  const ImportScreen({
    super.key,
    required this.pickerFactory,
    required this.onFilesSelected,
  });

  @override
  Widget build(BuildContext context) {
    return Center(
      child: ElevatedButton(
        key: const Key('import_button'),
        onPressed: () async {
          final files = await pickerFactory();
          onFilesSelected(files);
        },
        child: const Text('Import'),
      ),
    );
  }
}
```

The real (non-test) `pickerFactory` implementation, wired in Task 8's `main.dart`, uses `file_picker`:

```dart
import 'package:file_picker/file_picker.dart';

Future<List<Uint8List>> pickImagesFromDevice() async {
  final result = await FilePicker.platform.pickFiles(
    type: FileType.image,
    allowMultiple: true,
    withData: true,
  );
  if (result == null) return [];
  return result.files
      .where((f) => f.bytes != null)
      .map((f) => f.bytes!)
      .toList();
}
```

- [ ] **Step 5: Run it, confirm it passes**

Run: `cd app && flutter test test/import/import_screen_test.dart`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add app/pubspec.yaml app/pubspec.lock app/lib/import app/test/import
git commit -m "app: import screen for picking existing image files"
```

---

### Task 8: End-to-end wiring — capture/import → bridge → on-screen dimensions

**Files:**
- Modify: `app/lib/main.dart`, `crates/docscan-ffi/src/api.rs`
- Test: `app/test/end_to_end_test.dart`

**Interfaces:**
- Consumes: `decode_dimensions` (Task 3/4), `CaptureScreen`/`ImportScreen` (Tasks 6/7).
- Produces: the M1 acceptance deliverable itself — nothing further consumes this task.

- [ ] **Step 1: Regenerate bridge bindings for `decode_dimensions`**

`decode_dimensions` was already implemented in Task 3 but not yet exposed through codegen (Task 4 only generated `ping`). Run:

```bash
cd app && flutter_rust_bridge_codegen generate
```

Confirm the generated Dart file now also exposes a `decodeDimensions(bytes: ...)` function alongside `ping()`.

- [ ] **Step 2: Write the failing test**

`app/test/end_to_end_test.dart`:

```dart
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:opendocscan_app/bridge/frb_generated.dart';
import 'package:opendocscan_app/home_screen.dart';

void main() {
  setUpAll(() async {
    await RustLib.init();
  });

  testWidgets('picking a file shows its dimensions decoded via Rust',
      (tester) async {
    await tester.pumpWidget(MaterialApp(
      home: HomeScreen(
        pickerFactory: () async {
          // a 2x3 solid-color PNG's raw bytes, generated once and
          // checked into a fixture, e.g. app/test/fixtures/2x3.png
          return [await _fixtureBytes()];
        },
      ),
    ));

    await tester.tap(find.byKey(const Key('import_button')));
    await tester.pumpAndSettle();

    expect(find.text('2 x 3'), findsOneWidget);
  });
}

Future<Uint8List> _fixtureBytes() async {
  // loaded via package:flutter_test's asset bundle in the real test file;
  // see Step 3 for how the fixture itself is generated.
  throw UnimplementedError('replaced by the real fixture loader in Step 3');
}
```

- [ ] **Step 3: Generate the fixture and finish the test**

Add a tiny script (or a one-off `cargo run --example` in `docscan-core`) that writes a known 2×3 PNG to `app/test/fixtures/2x3.png`, then replace `_fixtureBytes` with:

```dart
import 'dart:io';

Future<Uint8List> _fixtureBytes() async {
  return File('test/fixtures/2x3.png').readAsBytesSync();
}
```

- [ ] **Step 4: Run it, confirm it fails**

Run: `cd app && flutter test test/end_to_end_test.dart`
Expected: FAIL — `HomeScreen` doesn't exist yet.

- [ ] **Step 5: Implement `HomeScreen` and wire it into `main.dart`**

`app/lib/home_screen.dart`:

```dart
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'bridge/frb_generated.dart';
import 'capture/capture_controller.dart';
import 'capture/capture_screen.dart';
import 'import/import_screen.dart';
import 'permissions/permission_gate.dart';

class HomeScreen extends StatefulWidget {
  final Future<List<Uint8List>> Function() pickerFactory;

  const HomeScreen({super.key, required this.pickerFactory});

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  String _status = 'no image yet';

  Future<void> _handleBytes(Uint8List bytes) async {
    final info = await decodeDimensions(bytes: bytes);
    setState(() => _status = '${info.width} x ${info.height}');
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('OpenDocScan')),
      body: Column(
        children: [
          Text(_status),
          Expanded(
            child: ImportScreen(
              pickerFactory: widget.pickerFactory,
              onFilesSelected: (files) {
                if (files.isNotEmpty) _handleBytes(files.first);
              },
            ),
          ),
        ],
      ),
    );
  }
}
```

Update `app/lib/main.dart` to use `HomeScreen` (with the real `pickImagesFromDevice` from Task 7 as its `pickerFactory`) as the app's home, replacing the placeholder `ping()`-only body from Task 4.

- [ ] **Step 6: Run it, confirm it passes**

Run: `cd app && flutter test test/end_to_end_test.dart`
Expected: PASS — proves a picked file's bytes cross the bridge and `decode_dimensions` reports the real dimensions.

- [ ] **Step 7: Run the entire Flutter + Rust test suite**

```bash
cargo test --workspace
cd app && flutter test
```

Expected: everything green.

- [ ] **Step 8: Commit**

```bash
git add app crates/docscan-ffi
git commit -m "app: end-to-end wiring, import screen decodes real image dimensions via Rust"
```

---

### Task 9: Real-device manual verification (not automated)

Camera behavior across vendors and the actual iOS/Android build pipeline cannot be fully proven by host-only `flutter test` — this monorepo's own history (OpenPhotoId, OpenPDFEdit) repeatedly found real-hardware-only bugs an all-green test suite missed. This task is a manual checklist, not code.

**Files:** none — this is a verification pass, log findings as new tasks/fixes if anything breaks.

- [ ] Build and run on a real Android device (not just an emulator): `cd app && flutter run -d <android-device-id>`. Grant camera + photos permissions when prompted. Confirm `ping()`'s result or the home screen loads without crashing.
- [ ] Tap the capture flow, take a real photo, confirm the app doesn't crash and (once Task 8's wiring is in the actual `main.dart` home route, not just the test) the decoded dimensions match the photo's real resolution.
- [ ] Use the import flow to pick a real photo from the device's gallery, confirm the same.
- [ ] Deny the camera permission when prompted and confirm the app shows a reasonable state (not a crash or a silently frozen spinner) — the `_ready` guard in `CaptureScreen` (Task 6) should be exercised for real here.
- [ ] Repeat the build+run on iOS Simulator: `cd app && flutter run -d "iPhone 15"` (or whatever simulator is available). Note: Simulator has no real camera — this only proves the import flow and the bridge on iOS; real camera capture on iOS needs a physical device pass, flag as outstanding if unavailable in this environment.
- [ ] Record any real-hardware-only findings (crashes, permission-flow surprises, performance issues) as follow-up tasks before M2 starts building the crop/correction UI on top of this foundation.

---

## Self-Review

**Spec coverage against `opendocscan/PLAN.md`'s M1 description** ("Flutter app scaffold; `flutter_rust_bridge`/`cargokit` integration for Android+iOS; camera capture screen; import screen; permissions flow; raw captured/imported image displayed end-to-end through the bridge"):
- Flutter scaffold → Task 2.
- Bridge/cargokit integration → Task 4.
- Camera capture screen → Task 6.
- Import screen → Task 7.
- Permissions flow → Task 5 (consumed by Tasks 6/7).
- Raw image round-tripping end-to-end through the bridge → Task 8.
- Real-device Android + iOS Simulator acceptance criteria from PLAN.md's M1 → Task 9.

**Placeholder scan:** no "TBD"/"add error handling"/"similar to Task N" patterns; the one deliberately deferred item (returning `Result` instead of panicking from `decode_dimensions`) is called out explicitly as a known gap to close before M2, not silently glossed over.

**Type consistency:** `DecodedInfo { width, height }` (Task 3) is what Task 8's `HomeScreen` reads (`info.width`/`info.height`); `CaptureController`'s `capture() -> Future<Uint8List>` (Task 6) matches what `CaptureScreen`'s `onCaptured` callback receives; `PermissionGate.request(AppPermission)` (Task 5) is the same signature both `CaptureScreen` (Task 6) and any future import-side permission check would use.

**Known open verification gap carried into M2:** exact `flutter_rust_bridge_codegen` CLI flags/generated-file naming (Task 4) should be confirmed against the actually-installed version before treating those steps as copy-paste-exact — flagged inline at the point they're used, consistent with how the M0 plan flagged the same class of risk for `imageproc`'s API surface.

---

**Next step after this plan**: M2 (edge detection wired to the live preview + interactive crop/correction screen) per `opendocscan/PLAN.md` — draft its own dedicated plan once M1 is merged and verified on real hardware (Task 9), since it depends on M1's actual `CaptureScreen`/bridge shapes being real, not assumed.
