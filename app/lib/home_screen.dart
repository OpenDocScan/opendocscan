import 'dart:typed_data';

import 'package:flutter/material.dart';

import 'capture/capture_controller.dart';
import 'capture/capture_screen.dart';
import 'import/file_picker_platform.dart';
import 'permissions/permission_gate.dart';
import 'scan_result.dart';
import 'theme.dart';

/// The M1 screen: capture or import a photograph, send its bytes through the
/// Rust core, and show what came back.
///
/// Everything the platform provides is injected, so the whole screen — both
/// entry points, the decode, and the failure path — runs in a host test with
/// no device attached.
typedef Decoder = Future<ScanResult> Function(Uint8List bytes);

class HomeScreen extends StatefulWidget {
  const HomeScreen({
    super.key,
    required this.picker,
    required this.permissions,
    required this.captureControllerFactory,
    this.decode = decodeThroughCore,
  });

  final FilePickerPlatform picker;
  final PermissionGate permissions;
  final CaptureController Function() captureControllerFactory;

  /// How a photograph becomes a result. Defaults to the real bridge; injected
  /// in host tests, where the Rust library is not loaded — `flutter test` runs
  /// on the Dart VM and the core is built for a phone, so a widget test that
  /// called across the bridge would fail on a missing symbol rather than on
  /// anything this screen does.
  final Decoder decode;

  @override
  State<HomeScreen> createState() => _HomeScreenState();
}

class _HomeScreenState extends State<HomeScreen> {
  final List<ScanResult> _results = [];
  bool _busy = false;

  Future<void> _handle(List<Uint8List> images) async {
    if (images.isEmpty) return; // a cancelled pick is not an error
    setState(() => _busy = true);

    final decoded = <ScanResult>[];
    for (final bytes in images) {
      decoded.add(await widget.decode(bytes));
    }

    if (!mounted) return;
    setState(() {
      _results.insertAll(0, decoded);
      _busy = false;
    });
  }

  Future<void> _import() async {
    final images = await widget.picker.pickImages();
    await _handle(images);
  }

  Future<void> _scan() async {
    final bytes = await Navigator.of(context).push<Uint8List>(
      MaterialPageRoute(
        builder: (_) => CaptureScreen(
          controller: widget.captureControllerFactory(),
          permissions: widget.permissions,
        ),
      ),
    );
    if (bytes != null) await _handle([bytes]);
  }

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);

    return Scaffold(
      appBar: AppBar(title: const Wordmark()),
      body: Column(
        children: [
          if (_busy) const LinearProgressIndicator(key: Key('busy')),
          Expanded(
            child: _results.isEmpty
                ? _Empty(theme: theme)
                : ListView.separated(
                    padding: const EdgeInsets.all(16),
                    itemCount: _results.length,
                    separatorBuilder: (_, _) => const SizedBox(height: 12),
                    itemBuilder: (_, i) => _ResultCard(result: _results[i]),
                  ),
          ),
          SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 16),
              child: Row(
                children: [
                  Expanded(
                    child: OutlinedButton(
                      key: const Key('import'),
                      onPressed: _busy ? null : _import,
                      child: const Text('Import images'),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: FilledButton(
                      key: const Key('scan'),
                      onPressed: _busy ? null : _scan,
                      child: const Text('Scan'),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _Empty extends StatelessWidget {
  const _Empty({required this.theme});

  final DocScanTheme theme;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              'Nothing scanned yet',
              style: TextStyle(
                color: theme.textStrong,
                fontSize: 20,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 10),
            Text(
              'Photograph a page or import one you already have. Everything '
              'happens on this device — nothing is uploaded.',
              textAlign: TextAlign.center,
              style: TextStyle(color: theme.textMuted, fontSize: 15),
            ),
          ],
        ),
      ),
    );
  }
}

class _ResultCard extends StatelessWidget {
  const _ResultCard({required this.result});

  final ScanResult result;

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);

    return Container(
      decoration: BoxDecoration(
        color: theme.surface,
        border: Border.all(color: theme.hairline),
        borderRadius: BorderRadius.circular(14),
      ),
      clipBehavior: Clip.antiAlias,
      child: switch (result) {
        ScanPending() => const Padding(
            padding: EdgeInsets.all(20),
            child: Center(child: CircularProgressIndicator()),
          ),
        ScanFailed(message: final message) => Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(Icons.error_outline, color: theme.danger),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Could not read this image',
                        style: TextStyle(
                          color: theme.textStrong,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        message,
                        key: const Key('failure-message'),
                        style: TextStyle(color: theme.textMuted, fontSize: 13),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ScanDecoded(
          bytes: final bytes,
          width: final width,
          height: final height,
        ) =>
          Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              // The bytes the core just decoded, rendered from the same buffer
              // that crossed the bridge — not a second read of the file.
              Image.memory(
                bytes,
                height: 220,
                fit: BoxFit.cover,
                gaplessPlayback: true,
              ),
              Padding(
                padding: const EdgeInsets.all(14),
                child: Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      '$width × $height',
                      key: const Key('dimensions'),
                      style: TextStyle(
                        color: theme.textStrong,
                        fontWeight: FontWeight.w600,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                    Text(
                      _readableSize(bytes.length),
                      style: TextStyle(color: theme.textMuted, fontSize: 13),
                    ),
                  ],
                ),
              ),
            ],
          ),
      },
    );
  }
}

String _readableSize(int bytes) {
  if (bytes < 1024) return '$bytes B';
  if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(0)} KB';
  return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
}
