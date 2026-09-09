#!/usr/bin/env python3
"""Take the one third-party endpoint out of the vendored account bundle.

The account elements ship a Nostr identity helper that fetches a proof from
`gist.github.com`. It is live code, not a comment, and this app cannot ship it:
the whole product claims that nothing reaches a third party, the end-to-end
suite greps the shipped files to assert it, and `deploy.sh` refuses to ship a
tree that names one.

Nothing in OpenDocScan calls that helper — the account page offers whatever
sign-in methods the server has configured, and the identity proof is a separate
feature of the Nostr flow. So the URL is rewritten to a same-origin path that
does not exist. If the helper is ever reached it fails with a 404 from our own
origin instead of quietly asking GitHub who someone is.

The service worker would block the request anyway; this is the second of the
two mechanisms, and it is the one that is true of the bytes on disk rather than
of the runtime. Same reasoning, and the same shape, as `deorbit-tesseract.py`.

Re-run after re-vendoring the bundle:

    python3 scripts/deorbit-openapps.py
"""

import pathlib
import re
import sys

VENDOR = pathlib.Path(__file__).resolve().parent.parent / "vendor" / "openapps"

REWRITES = [
    # The Nostr identity-proof fetch. Rewritten, not deleted, so the call site
    # keeps its shape and the bundle stays syntactically identical.
    ("https://gist.github.com/", "/openapps-disabled/gist/"),
]


def main() -> int:
    if not VENDOR.is_dir():
        print(f"no vendored bundle at {VENDOR}", file=sys.stderr)
        return 1

    changed = 0
    for path in sorted(VENDOR.glob("*.js")):
        text = path.read_text(encoding="utf-8")
        original = text
        for old, new in REWRITES:
            text = text.replace(old, new)
        if text != original:
            path.write_text(text, encoding="utf-8")
            print(f"  rewrote {path.name}")
            changed += 1

    # The same rule the end-to-end suite and deploy.sh apply, so this script
    # agrees with the checks that would otherwise refuse the deploy. Matching
    # them loosely is how a script reports clean and the guard still fires.
    allowed = re.compile(r"^(localhost|127\.0\.0\.1|(www\.)?w3\.org|schema\.org)$")
    remaining = set()
    for path in sorted(VENDOR.glob("*.js")):
        text = path.read_text(encoding="utf-8")
        for host in re.findall(r"https?://([a-zA-Z0-9.-]+)", text):
            if not allowed.match(host):
                remaining.add(host)

    if remaining:
        print("still naming external hosts: " + ", ".join(sorted(remaining)), file=sys.stderr)
        return 1

    print(f"clean — {changed} file(s) rewritten, no external host remains")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
