import 'package:permission_handler/permission_handler.dart';

/// Asking for a permission, behind an interface.
///
/// The interface exists so the screens can be tested on the host. A real
/// `Permission.camera.request()` needs a platform channel, which means a
/// device or an emulator, which means the camera screen would have no host
/// tests at all — and the camera screen is exactly where the interesting
/// states live (denied once, denied permanently, granted).
abstract class PermissionGate {
  Future<bool> request(Permission permission);

  /// True when the user chose "don't ask again" (or iOS's equivalent). The
  /// distinction matters: a plain denial can be re-requested, this one can
  /// only be undone in Settings, and offering "try again" for it is a button
  /// that visibly does nothing.
  Future<bool> isPermanentlyDenied(Permission permission);
}

class SystemPermissionGate implements PermissionGate {
  const SystemPermissionGate();

  @override
  Future<bool> request(Permission permission) async {
    final status = await permission.request();
    return status.isGranted || status.isLimited;
  }

  @override
  Future<bool> isPermanentlyDenied(Permission permission) =>
      permission.isPermanentlyDenied;
}
