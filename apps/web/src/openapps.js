// The account server, under this product's own name.
//
// These two constants are defined here and nowhere else, and everything that
// needs them imports them. That is not tidiness: OpenCapture kept its base URL
// as a literal in two places, moved to its own domain, fixed one, and shipped
// the other still pointing at the platform's host for months. A grep for the
// platform's domain outside this file must return nothing, and the end-to-end
// suite asserts exactly that.
//
// Both hostnames, not just the first. `auth.` is a URL someone may glance at
// during sign-in; `gateway.` is the one a browser interrupts them to ask about.
// Masking only `auth.` is the easy half and the half that matters least.

export const OPENAPPS_BASE_URL = 'https://auth.opendocscan.com';
export const OPENAPPS_GATEWAY_URL = 'https://gateway.opendocscan.com';

/// Where the account lives. One route, named for the destination rather than
/// for the action — this is where a signed-in visitor goes to see their
/// balance, so it is `/account` and never `/login?next=…`.
export const ACCOUNT_PATH = '/account';
