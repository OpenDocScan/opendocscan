import 'dart:async';

import 'package:app_links/app_links.dart';
import 'package:url_launcher/url_launcher.dart';

import 'openapps.dart';

/// Opening a URL somewhere outside this app.
///
/// A seam, so the sign-in flow can be driven in a host test with no browser and
/// no device: the test asserts *which* URL was opened, which is where the
/// interesting mistakes live.
abstract interface class Browser {
  Future<bool> open(Uri url);
}

/// The real browser — Chrome, Safari, whatever the phone is set to.
///
/// `externalApplication`, never an in-app web view. That is not a preference:
/// **Google refuses OAuth from an embedded user agent** and answers
/// `disallowed_useragent`, so a WebView-based sign-in cannot be made to work at
/// all. It is also what RFC 8252 asks for — the system browser already holds
/// the user's Google session, so most sign-ins are one tap rather than a
/// password typed into a window this app could have been reading.
class SystemBrowser implements Browser {
  const SystemBrowser();

  @override
  Future<bool> open(Uri url) =>
      launchUrl(url, mode: LaunchMode.externalApplication);
}

/// Links the operating system hands this app after the browser is done.
abstract interface class DeepLinks {
  /// The link that launched a cold start, if one did.
  Future<Uri?> initial();

  /// Links that arrive while the app is already running — the warm case, and
  /// the common one, since signing in leaves the app alive in the background.
  Stream<Uri> get stream;
}

class SystemDeepLinks implements DeepLinks {
  SystemDeepLinks([AppLinks? links]) : _links = links ?? AppLinks();

  final AppLinks _links;

  @override
  Future<Uri?> initial() => _links.getInitialLink();

  @override
  Stream<Uri> get stream => _links.uriLinkStream;
}

/// The one-time code carried by a callback URI, or null if this is not one.
///
/// Reads the **fragment first, then the query**, which is the same order the
/// web SDK uses and is not arbitrary: the account server's OAuth callback puts
/// the code in the fragment — its own sign-in page says so in as many words —
/// and a fragment never reaches a server, which is exactly why it is safe to
/// carry a one-time code through a redirect chain. Checking the query as well
/// costs one line and means a future server that switches still works here.
String? codeFrom(Uri uri) {
  if (uri.scheme != kCallbackScheme || uri.host != kCallbackHost) return null;
  final fragment = Uri.splitQueryString(uri.fragment);
  return fragment['code'] ?? uri.queryParameters['code'];
}
