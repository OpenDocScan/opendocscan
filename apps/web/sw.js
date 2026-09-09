// Offline support, and a hard stop on anything leaving this device.
//
// The second job is the interesting one. "Nothing is uploaded" is usually a
// promise you are asked to take on faith. Here every request the page makes
// passes through this worker, and any request to another origin is refused
// outright — so the guarantee is enforced by the app rather than asserted
// by its marketing. If a future dependency ever tried to phone home, it
// would fail loudly here instead of succeeding quietly.

const VERSION = 'v4';
const SHELL_CACHE = `opendocscan-shell-${VERSION}`;
const ASSET_CACHE = `opendocscan-assets-${VERSION}`;

// The smallest set that gets someone from a cold start to a finished scan
// with no network. Deliberately excludes the OCR engine, which is ~6MB —
// that is fetched the first time text recognition actually runs, and cached
// from then on.
const SHELL = [
  './',
  'index.html',
  'account.html',
  'manifest.webmanifest',
  // The design tokens, and the two faces they name. Missing from the shell,
  // the app comes back from a cold offline start unstyled and in Times — which
  // reads as a broken build rather than as a missing stylesheet.
  'tokens/css/colors.css',
  'tokens/css/typography.css',
  'tokens/css/spacing.css',
  'tokens/css/radius.css',
  'tokens/css/motion.css',
  'tokens/css/fonts.css',
  'tokens/fonts/Geist-Regular.woff2',
  'tokens/fonts/Geist-Medium.woff2',
  'src/styles.css',
  'src/app.js',
  'src/store.js',
  'src/scanner.js',
  // The worker is a separate document to the browser and is fetched on
  // its own; leaving it out of the shell is how the app works offline
  // right up until the moment someone tries to scan something.
  'src/scanner-worker.js',
  'src/ocr.js',
  'src/wasm/docscan.js',
  'src/wasm/docscan_bg.wasm',
  'icons/icon.svg',
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(SHELL_CACHE);
      // Individually rather than `addAll`, which rejects the whole install
      // if any single entry 404s — one missing icon should not leave the
      // app with no offline support at all.
      await Promise.all(
        SHELL.map((path) =>
          cache.add(new Request(path, { cache: 'reload' })).catch((error) => {
            console.warn('[sw] could not precache', path, error);
          }),
        ),
      );
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const keep = new Set([SHELL_CACHE, ASSET_CACHE]);
      for (const name of await caches.keys()) {
        if (name.startsWith('opendocscan-') && !keep.has(name)) await caches.delete(name);
      }
      await self.clients.claim();
    })(),
  );
});

// The account server, and the only cross-origin host this app may reach.
//
// Kept in step with `src/openapps.js`, which is the source of truth — a service
// worker cannot import from it, so the end-to-end suite asserts the two agree
// rather than trusting anyone to remember. It is only reachable from the
// account page; see below.
const ACCOUNT_ORIGIN = 'https://auth.opendocscan.com';
const ACCOUNT_PAGES = ['/account', '/account.html'];

function blocked(href) {
  console.warn('[sw] blocked a cross-origin request:', href);
  return new Response('OpenDocScan does not make requests to other servers.', {
    status: 403,
    statusText: 'Blocked by OpenDocScan',
  });
}

// The account host is allowed, but only for the page whose whole job is the
// account. Allowing it origin-wide would mean the scanner could reach it, and
// the scanner reaching any server at all is the thing this worker exists to
// prevent. Which page asked is knowable: the fetch event names its client.
async function accountRequest(event, request) {
  const client = event.clientId ? await self.clients.get(event.clientId) : null;
  const from = client ? new URL(client.url).pathname : '';
  if (!ACCOUNT_PAGES.includes(from)) return blocked(request.url);
  return fetch(request);
}

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // The enforcement, and it covers every method. It used to return early on
  // anything but GET, which left a POST to another origin untouched — so the
  // guarantee the README describes was true of reads and not of writes. Sign-in
  // is exactly a cross-origin POST, so that gap had to close in the same change
  // that opened a hole in it deliberately.
  if (url.origin !== self.location.origin) {
    if (url.origin === ACCOUNT_ORIGIN) {
      event.respondWith(accountRequest(event, request));
      return;
    }
    event.respondWith(blocked(request.url));
    return;
  }

  // Same-origin writes are not this worker's business, and caching them would
  // be wrong.
  if (request.method !== 'GET') return;

  event.respondWith(serve(request));
});

async function serve(request) {
  const cached = await caches.match(request, { ignoreSearch: true });
  if (cached) return cached;

  try {
    const response = await fetch(request);
    // Only complete, successful, same-origin responses are worth keeping.
    // Caching an opaque or partial response is how an app ends up serving
    // a truncated WASM module from cache forever.
    if (response.ok && response.type === 'basic') {
      const cache = await caches.open(ASSET_CACHE);
      cache.put(request, response.clone());
    }
    return response;
  } catch (error) {
    // Offline and not cached. For a navigation that means the app shell,
    // which is always cached, so the app still opens.
    if (request.mode === 'navigate') {
      const shell = await caches.match('index.html');
      if (shell) return shell;
    }
    throw error;
  }
}
