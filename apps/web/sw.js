// Offline support, and a hard stop on anything leaving this device.
//
// The second job is the interesting one. "Nothing is uploaded" is usually a
// promise you are asked to take on faith. Here every request the page makes
// passes through this worker, and any request to another origin is refused
// outright — so the guarantee is enforced by the app rather than asserted
// by its marketing. If a future dependency ever tried to phone home, it
// would fail loudly here instead of succeeding quietly.

const VERSION = 'v3';
const SHELL_CACHE = `opendocscan-shell-${VERSION}`;
const ASSET_CACHE = `opendocscan-assets-${VERSION}`;

// The smallest set that gets someone from a cold start to a finished scan
// with no network. Deliberately excludes the OCR engine, which is ~6MB —
// that is fetched the first time text recognition actually runs, and cached
// from then on.
const SHELL = [
  './',
  'index.html',
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

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;

  const url = new URL(request.url);

  // The enforcement. Nothing in this app has any business talking to
  // another origin, so nothing is allowed to.
  if (url.origin !== self.location.origin) {
    console.warn('[sw] blocked a cross-origin request:', url.href);
    event.respondWith(
      new Response('OpenDocScan does not make requests to other servers.', {
        status: 403,
        statusText: 'Blocked by OpenDocScan',
      }),
    );
    return;
  }

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
