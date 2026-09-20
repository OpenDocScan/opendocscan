/// The account server, under this product's own name.
///
/// The Dart twin of `apps/web/src/openapps.js`, and it exists for the same
/// reason: these hostnames are defined **here and nowhere else**. OpenCapture
/// kept its base URL as a literal in two places, moved to its own domain, fixed
/// one, and shipped the other pointing at the platform's host for months. A
/// search for the platform's domain anywhere else under `lib/` must come back
/// empty, and `test/account_masking_test.dart` asserts exactly that.
///
/// Both hostnames, not just the first. `auth.` is a URL someone may glance at
/// during sign-in; on a phone `gateway.` would be the one an OS permission
/// sheet names. Masking only `auth.` is the easy half and the half that
/// matters least.
library;

/// Where accounts, sessions and the credit ledger live.
const String kAuthBase = 'https://auth.opendocscan.com';

/// Where paid features would live. Nothing in this app calls it, and that is
/// not an oversight: every operation OpenDocScan performs — detection,
/// rectification, filtering, recognition, PDF assembly — runs on the phone's
/// own processor and costs us nothing, so there is nothing to meter. The
/// constant is here so that the masking rule has both halves to check, and so
/// the day a server-side feature appears nobody reaches for a literal.
const String kGatewayBase = 'https://gateway.opendocscan.com';

/// The scheme the browser hands the finished sign-in back on.
///
/// A native app has no page for an OAuth redirect to land on, so the round trip
/// ends at the operating system instead: the browser is sent to
/// `opendocscan://auth#code=…`, Android and iOS match that against the
/// declarations in `AndroidManifest.xml` and `Info.plist`, and the app is woken
/// with the whole URI, fragment included.
const String kCallbackScheme = 'opendocscan';
const String kCallbackHost = 'auth';

/// Where the sign-in round trip is told to come back to.
///
/// **Not the deep link itself, and that is deliberate.** The server validates
/// `return_to` against `allowed_origins` by exact origin. `opendocscan://auth`
/// parses to a perfectly good origin — measured, it comes back
/// `400 … "return_to origin opendocscan://auth is not in allowed_origins"`
/// rather than "not a valid absolute URL" — so pointing straight at the deep
/// link needs one entry added to `/opt/openapps/deploy/prod.env`. That file is
/// shared by every product in the suite and applying it recreates the
/// container, which takes sign-in down for all of them for a few seconds.
///
/// This product's own account page is already on the list (measured: 307), so
/// the browser lands there instead and that page forwards the one-time code on
/// to the app. It costs one redirect and buys three things: no shared restart,
/// nothing for a future operator to forget, and — because the page is ours — a
/// real fallback. On a device with no app installed the custom-scheme
/// navigation simply does nothing, and the visitor is left signed in on the web
/// page they are already looking at, which is the best available outcome rather
/// than a dead end.
///
/// If that allow-list entry is ever added, this becomes
/// `'$kCallbackScheme://$kCallbackHost'` and the trampoline in
/// `apps/web/account.html` can go. It is one line, on purpose.
const String kSignInReturnTo = 'https://opendocscan.com/account?native=1';

/// The one host this app is allowed to open a socket to.
///
/// Android used to enforce this for us: the release manifest shipped without
/// `android.permission.INTERNET`, so the process could not open a socket at
/// all. An account needs one, so that guarantee had to be replaced rather than
/// simply dropped — this predicate is the replacement, it is checked on every
/// single request in `AccountApi`, and `test/account_api_test.dart` proves a
/// request to any other host throws instead of leaving the device.
///
/// It is the same shape as the rule the web app's service worker enforces with
/// a 403, and it is enforced in the same place: at the one door.
bool isAccountHost(Uri url) =>
    url.scheme == 'https' && url.host == Uri.parse(kAuthBase).host;
