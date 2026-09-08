#!/usr/bin/env python3
"""Serve this directory: over HTTPS to the local network, or plainly to localhost.

    python3 scripts/serve-lan.py            HTTPS on every interface, :8443
    python3 scripts/serve-lan.py --plain    HTTP on loopback, :8765

One server rather than two, because both need the same three things the
stock `http.server` gets wrong for this app — the WebAssembly MIME type,
the precompressed `.br`/`.gz` siblings the build produces, and no caching
while a rebuild is one keystroke away. The plain mode is for this machine
and for the test suite; the HTTPS mode is for a phone.

Testing on a real phone needs two things loopback does not give you:

  * The phone has to be able to reach the machine, so this binds to every
    interface rather than to loopback.
  * The origin has to be a *secure context*. `getUserMedia` is gated on it,
    and both Safari and Chrome go further than the spec requires: they
    refuse camera access on any origin whose certificate did not validate,
    even one the user clicked through. So a self-signed certificate is not
    enough on its own — it has to actually be trusted by the phone. The
    banner this prints says how.

Everything here is for development. Deploying is copying this directory to
any static host that speaks HTTPS.
"""

import http.server
import ipaddress
import mimetypes
import os
import socket
import ssl
import subprocess
import sys
from pathlib import Path

WEB_DIR = Path(__file__).resolve().parent.parent
CERT_DIR = WEB_DIR / ".certs"
CERT = CERT_DIR / "dev-cert.pem"
KEY = CERT_DIR / "dev-key.pem"
PLAIN = "--plain" in sys.argv
PORT = int(os.environ.get("PORT", "8765" if PLAIN else "8443"))

# Python's mimetypes table predates WebAssembly on some installs, and a
# .wasm served as application/octet-stream fails instantiateStreaming with
# an error that points nowhere near the real cause.
mimetypes.add_type("application/wasm", ".wasm")
mimetypes.add_type("text/javascript", ".mjs")
mimetypes.add_type("application/manifest+json", ".webmanifest")


def lan_address() -> str:
    """This machine's address on the LAN.

    Opening a UDP socket towards a public address makes the routing table
    pick the interface that would actually carry traffic, without sending
    anything. Reading the first hostname resolution instead tends to answer
    127.0.0.1 or a stale VPN address.
    """
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("192.0.2.1", 1))  # TEST-NET-1: reserved, never routed
        return probe.getsockname()[0]
    except OSError:
        return "127.0.0.1"
    finally:
        probe.close()


def ensure_certificate(host: str) -> None:
    if CERT.exists() and KEY.exists():
        return

    CERT_DIR.mkdir(exist_ok=True)
    (CERT_DIR / ".gitignore").write_text("*\n")

    local_name = subprocess.run(
        ["scutil", "--get", "LocalHostName"], capture_output=True, text=True, check=False
    ).stdout.strip()

    names = ["DNS:localhost", "IP:127.0.0.1", f"IP:{host}"]
    if local_name:
        names.append(f"DNS:{local_name}.local")

    config = "\n".join(
        [
            "[req]",
            "distinguished_name = dn",
            "x509_extensions = ext",
            "prompt = no",
            "[dn]",
            "CN = OpenDocScan dev",
            "[ext]",
            "basicConstraints = critical, CA:FALSE",
            "keyUsage = critical, digitalSignature, keyEncipherment",
            # Apple platforms reject a server certificate that does not say
            # it is one, however else it is trusted.
            "extendedKeyUsage = serverAuth",
            f"subjectAltName = {', '.join(names)}",
        ]
    )

    print("Generating a development certificate…", flush=True)
    subprocess.run(
        [
            "openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", str(KEY), "-out", str(CERT),
            # Safari rejects server certificates valid for more than 398
            # days, and does so with a generic error that looks like an
            # unrelated failure.
            "-days", "397",
            "-sha256", "-config", "/dev/stdin",
        ],
        input=config,
        text=True,
        check=True,
        capture_output=True,
    )
    print(f"Wrote {CERT.name} and {KEY.name} to {CERT_DIR}", flush=True)


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(WEB_DIR), **kwargs)

    def do_GET(self):
        # The phone has to fetch the certificate from somewhere, and it
        # cannot be the certificate directory — that is deliberately not
        # inside the served tree, so a real deployment of this folder can
        # never accidentally publish a private key.
        if self.path.split("?")[0] in ("/dev-cert.pem", "/dev-cert.crt"):
            self.serve_certificate()
            return
        if self.serve_precompressed():
            return
        super().do_GET()

    def serve_precompressed(self) -> bool:
        """Serve `<file>.br` or `<file>.gz` when the client will take it.

        The build writes these next to the originals, and over a phone
        connection they are most of what the first visit costs: the wasm
        module is 318KB raw and 114KB brotli. Doing it here rather than
        compressing per request is also how a static host would do it —
        this is a development server standing in for one, and it should
        not make the app look faster than the real thing will.
        """
        path = self.path.split("?")[0]
        if not path.endswith((".wasm", ".js", ".css", ".json", ".webmanifest")):
            return False

        accepted = self.headers.get("Accept-Encoding", "")
        target = self.translate_path(path)
        for suffix, encoding in ((".br", "br"), (".gz", "gzip")):
            if encoding not in accepted:
                continue
            candidate = Path(target + suffix)
            if not candidate.is_file():
                continue
            body = candidate.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", self.guess_type(target))
            self.send_header("Content-Encoding", encoding)
            self.send_header("Content-Length", str(len(body)))
            # Two representations of one URL. Without this a proxy — or the
            # browser's own cache — can hand the brotli bytes to a client
            # that asked for gzip.
            self.send_header("Vary", "Accept-Encoding")
            self.end_headers()
            self.wfile.write(body)
            return True
        return False

    def serve_certificate(self):
        body = CERT.read_bytes()
        self.send_response(200)
        # iOS only offers to install a downloaded file as a configuration
        # profile when it arrives under this type; served as text/plain it
        # is displayed as gibberish instead.
        self.send_header("Content-Type", "application/x-x509-ca-cert")
        self.send_header("Content-Disposition", 'attachment; filename="opendocscan-dev.crt"')
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def end_headers(self):
        # No caching in development, or a rebuilt .wasm keeps losing to the
        # copy the phone already has.
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, fmt, *args):
        if "GET" in fmt % args and " 200 " not in fmt % args:
            super().log_message(fmt, *args)


def main() -> int:
    if PLAIN:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
        print(f"OpenDocScan is serving at http://127.0.0.1:{PORT}/", flush=True)
        print("No camera here — http on a non-localhost origin is not a secure", flush=True)
        print("context. Use `npm run serve:lan` for a phone. Ctrl-C to stop.", flush=True)
        return run(server)

    host = lan_address()
    try:
        ipaddress.ip_address(host)
    except ValueError:
        print(f"Could not work out a LAN address (got {host!r}).", file=sys.stderr)
        return 1

    ensure_certificate(host)

    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(certfile=CERT, keyfile=KEY)

    server = http.server.ThreadingHTTPServer(("0.0.0.0", PORT), Handler)
    server.socket = context.wrap_socket(server.socket, server_side=True)

    url = f"https://{host}:{PORT}/"
    print(
        f"""
  OpenDocScan is serving at

      {url}

  Before the camera will work, the phone has to TRUST this certificate.
  Both Safari and Chrome refuse getUserMedia on an origin whose
  certificate failed to validate — including one you clicked through — so
  tapping "visit anyway" gets you the app with a dead shutter button.

  iPhone / iPad
    1. Open {url}dev-cert.pem in Safari and allow the profile download.
    2. Settings > General > VPN & Device Management > install it.
    3. Settings > General > About > Certificate Trust Settings >
       switch on "OpenDocScan dev".
    4. Open {url}

  Android
    1. Open {url}dev-cert.pem to download it.
    2. Settings > Security > Encryption & credentials >
       Install a certificate > CA certificate.
    3. Open {url}

  Both devices must be on the same network, and some routers block
  device-to-device traffic on guest Wi-Fi.

  Without the camera, everything else still works over plain HTTP —
  "Import images" covers the whole pipeline. Ctrl-C to stop.
""",
        flush=True,
    )

    return run(server)


def run(server: http.server.ThreadingHTTPServer) -> int:
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print("\nStopped.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
