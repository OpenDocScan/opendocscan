import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';

/// Picking image files, behind an interface, so import has host tests.
abstract class FilePickerPlatform {
  /// The bytes of whatever the user picked. Empty when they cancelled —
  /// cancelling is not an error and must not reach the UI as one.
  Future<List<Uint8List>> pickImages();
}

class SystemFilePicker implements FilePickerPlatform {
  const SystemFilePicker();

  @override
  Future<List<Uint8List>> pickImages() async {
    // file_picker 12 made `pickFiles` static and non-nullable — a cancelled
    // pick is an empty list, not null — and deprecated `withData` in favour of
    // reading each file explicitly. Reading per file also keeps one unreadable
    // pick from taking the rest of the selection down with it.
    final picked = await FilePicker.pickFiles(
      type: FileType.image,
      dialogTitle: 'Choose pages to scan',
    );

    final images = <Uint8List>[];
    for (final file in picked) {
      try {
        images.add(await file.readAsBytes());
      } catch (_) {
        // A file the platform listed but will not hand over — a cloud item
        // that failed to download, most often. Skipping it is right: the core
        // reports undecodable *bytes*, and there are none to report on.
        continue;
      }
    }
    return images;
  }
}
