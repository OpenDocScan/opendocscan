import 'package:flutter/material.dart';

import 'capture/capture_controller.dart';
import 'home_screen.dart';
import 'import/file_picker_platform.dart';
import 'permissions/permission_gate.dart';
import 'src/rust/frb_generated.dart';
import 'theme.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // Loads the Rust library and runs `init_app`. Everything that crosses the
  // bridge throws until this has completed, so it is awaited rather than
  // fired off — a race here looks like a missing symbol.
  await RustLib.init();
  runApp(const DocScanApp());
}

class DocScanApp extends StatelessWidget {
  const DocScanApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'OpenDocScan',
      debugShowCheckedModeBanner: false,
      theme: buildTheme(Brightness.light),
      darkTheme: buildTheme(Brightness.dark),
      home: HomeScreen(
        picker: const SystemFilePicker(),
        permissions: const SystemPermissionGate(),
        captureControllerFactory: DeviceCaptureController.new,
      ),
    );
  }
}
