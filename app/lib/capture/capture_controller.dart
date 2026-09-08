import 'dart:typed_data';

import 'package:camera/camera.dart';
import 'package:flutter/widgets.dart';

/// Taking a photograph, behind an interface.
///
/// Everything above this returns bytes, so the screens never touch a
/// `CameraController` and can be driven by a fake in a host test.
abstract class CaptureController {
  /// Get the camera ready. Safe to call more than once.
  Future<void> initialise();

  /// The live preview, or null before [initialise] has completed.
  Widget? preview();

  /// One photograph, as encoded bytes. JPEG on both platforms — which is what
  /// makes the Rust side's `jpeg` feature load-bearing rather than decorative.
  Future<Uint8List> capture();

  Future<void> dispose();
}

class DeviceCaptureController implements CaptureController {
  CameraController? _controller;

  @override
  Future<void> initialise() async {
    if (_controller != null) return;

    final cameras = await availableCameras();
    if (cameras.isEmpty) {
      throw StateError('This device reports no cameras.');
    }

    // The rear camera, explicitly. `cameras.first` is the front one on a
    // good number of Android devices, and a document scanner that opens
    // facing the user reads as broken before anyone has taken a picture.
    final rear = cameras.firstWhere(
      (c) => c.lensDirection == CameraLensDirection.back,
      orElse: () => cameras.first,
    );

    final controller = CameraController(
      rear,
      ResolutionPreset.veryHigh,
      enableAudio: false, // a scanner has no reason to open the microphone
      imageFormatGroup: ImageFormatGroup.jpeg,
    );
    await controller.initialize();
    _controller = controller;
  }

  @override
  Widget? preview() {
    final controller = _controller;
    if (controller == null || !controller.value.isInitialized) return null;
    return CameraPreview(controller);
  }

  @override
  Future<Uint8List> capture() async {
    final controller = _controller;
    if (controller == null || !controller.value.isInitialized) {
      throw StateError('capture() before initialise() completed');
    }
    final file = await controller.takePicture();
    return file.readAsBytes();
  }

  @override
  Future<void> dispose() async {
    await _controller?.dispose();
    _controller = null;
  }
}
