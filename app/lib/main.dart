import 'package:flutter/material.dart';

import 'account/account_api.dart';
import 'account/account_controller.dart';
import 'account/session.dart';
import 'account/sign_in.dart';
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

class DocScanApp extends StatefulWidget {
  const DocScanApp({super.key});

  @override
  State<DocScanApp> createState() => _DocScanAppState();
}

class _DocScanAppState extends State<DocScanApp> {
  late final AccountController _account;

  @override
  void initState() {
    super.initState();
    _account = AccountController(
      api: AccountApi(store: const SecureSessionStore()),
      links: SystemDeepLinks(),
    );
    // Started here rather than from the account screen, and started at launch
    // rather than on first visit. Two reasons, both about the return trip:
    // signing in leaves the app, so the callback often arrives as a *cold
    // start* with nothing mounted, and a listener attached later would miss
    // it entirely. It also means the account control shows the right state
    // before anybody taps it.
    _account.start();
  }

  @override
  void dispose() {
    _account.dispose();
    super.dispose();
  }

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
        account: _account,
      ),
    );
  }
}
