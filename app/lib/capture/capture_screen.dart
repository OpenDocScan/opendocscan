import 'dart:typed_data';

import 'package:flutter/material.dart';
import 'package:permission_handler/permission_handler.dart';

import '../permissions/permission_gate.dart';
import '../theme.dart';
import 'capture_controller.dart';

/// The viewfinder.
///
/// Pops with the captured bytes, or with null if the user backed out. Both the
/// camera and the permission check arrive injected, so every state below can
/// be driven in a host test — including the two that are awkward to reach on a
/// real device on purpose.
class CaptureScreen extends StatefulWidget {
  const CaptureScreen({
    super.key,
    required this.controller,
    required this.permissions,
  });

  final CaptureController controller;
  final PermissionGate permissions;

  @override
  State<CaptureScreen> createState() => _CaptureScreenState();
}

enum _Stage { asking, denied, deniedForever, ready, failed }

class _CaptureScreenState extends State<CaptureScreen> {
  _Stage _stage = _Stage.asking;
  String _error = '';
  bool _capturing = false;

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    setState(() => _stage = _Stage.asking);

    final granted = await widget.permissions.request(Permission.camera);
    if (!mounted) return;

    if (!granted) {
      // "Denied" and "denied permanently" need different words and different
      // buttons: only one of them can be undone by asking again.
      final forever =
          await widget.permissions.isPermanentlyDenied(Permission.camera);
      if (!mounted) return;
      setState(() => _stage = forever ? _Stage.deniedForever : _Stage.denied);
      return;
    }

    try {
      await widget.controller.initialise();
      if (!mounted) return;
      setState(() => _stage = _Stage.ready);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _stage = _Stage.failed;
        _error = e is StateError ? e.message : e.toString();
      });
    }
  }

  Future<void> _shutter() async {
    if (_capturing) return; // a double tap must not take two photographs
    setState(() => _capturing = true);
    try {
      final Uint8List bytes = await widget.controller.capture();
      if (!mounted) return;
      Navigator.of(context).pop(bytes);
    } catch (e) {
      if (!mounted) return;
      setState(() {
        _stage = _Stage.failed;
        _error = e.toString();
        _capturing = false;
      });
    }
  }

  @override
  void dispose() {
    widget.controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final theme = DocScanTheme.of(context);

    return Scaffold(
      backgroundColor: DocScanTokens.backgroundDark,
      appBar: AppBar(
        backgroundColor: DocScanTokens.backgroundDark,
        foregroundColor: DocScanTokens.white,
        title: const Text('Scan'),
      ),
      body: switch (_stage) {
        _Stage.asking => const Center(child: CircularProgressIndicator()),
        _Stage.denied => _Message(
            key: const Key('capture-denied'),
            title: 'The camera is off for OpenDocScan',
            body: 'Scanning needs the camera. Nothing it sees leaves your '
                'device — the photograph is processed here and never uploaded.',
            actionLabel: 'Ask again',
            onAction: _start,
          ),
        _Stage.deniedForever => _Message(
            key: const Key('capture-denied-forever'),
            title: 'The camera is blocked in Settings',
            body: 'Asking again will not do anything from here — the permission '
                'has to be turned back on in the system settings.',
            actionLabel: 'Open settings',
            onAction: openAppSettings,
          ),
        _Stage.failed => _Message(
            key: const Key('capture-failed'),
            title: 'The camera would not start',
            body: _error,
            actionLabel: 'Try again',
            onAction: _start,
          ),
        _Stage.ready => Column(
            children: [
              Expanded(
                child: Container(
                  color: DocScanTokens.black,
                  width: double.infinity,
                  child: widget.controller.preview() ??
                      const Center(child: CircularProgressIndicator()),
                ),
              ),
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 28),
                child: Semantics(
                  button: true,
                  label: 'Take the photograph',
                  child: GestureDetector(
                    key: const Key('shutter'),
                    onTap: _shutter,
                    child: Container(
                      width: 74,
                      height: 74,
                      decoration: BoxDecoration(
                        // Green means action, and this is the action.
                        color: _capturing
                            ? theme.brand.withValues(alpha: 0.5)
                            : theme.brand,
                        shape: BoxShape.circle,
                        border: Border.all(
                          color: DocScanTokens.white,
                          width: 4,
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ],
          ),
      },
    );
  }
}

class _Message extends StatelessWidget {
  const _Message({
    super.key,
    required this.title,
    required this.body,
    required this.actionLabel,
    required this.onAction,
  });

  final String title;
  final String body;
  final String actionLabel;
  final VoidCallback onAction;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(28),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              title,
              textAlign: TextAlign.center,
              style: const TextStyle(
                color: DocScanTokens.white,
                fontSize: 20,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 12),
            Text(
              body,
              textAlign: TextAlign.center,
              style: const TextStyle(color: Color(0x99FFFFFF), fontSize: 15),
            ),
            const SizedBox(height: 24),
            FilledButton(onPressed: onAction, child: Text(actionLabel)),
          ],
        ),
      ),
    );
  }
}
