// The document library: everything the user has ever scanned, held in
// IndexedDB in this browser and nowhere else.
//
// There is no sync, no account, and no server — which removes the entire
// class of failure the report found users complaining about ("免费版导出全是
// 水印，逼着订阅", "强制要求登录账号"), and introduces exactly one in its
// place: a browser that discards site data reclaims the library. That is
// what `requestPersistence` is for, and why the UI tells the user the
// answer rather than hiding it.

const DB_NAME = 'opendocscan';
const DB_VERSION = 1;

let dbPromise = null;

function openDb() {
  if (dbPromise) return dbPromise;

  dbPromise = new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION);

    request.onupgradeneeded = () => {
      const db = request.result;

      if (!db.objectStoreNames.contains('documents')) {
        db.createObjectStore('documents', { keyPath: 'id' });
      }
      if (!db.objectStoreNames.contains('pages')) {
        const pages = db.createObjectStore('pages', { keyPath: 'id' });
        // Pages are only ever read a whole document at a time, so this is
        // the one index worth carrying.
        pages.createIndex('docId', 'docId', { unique: false });
      }
      if (!db.objectStoreNames.contains('settings')) {
        db.createObjectStore('settings', { keyPath: 'key' });
      }
    };

    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
    request.onblocked = () =>
      reject(new Error('the library is open in another tab — close it and retry'));
  });

  return dbPromise;
}

function tx(db, stores, mode) {
  const transaction = db.transaction(stores, mode);
  const done = new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onerror = () => reject(transaction.error);
    transaction.onabort = () => reject(transaction.error ?? new Error('transaction aborted'));
  });
  return { transaction, done };
}

function request(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export function newId() {
  // randomUUID needs a secure context, which this app requires anyway —
  // getUserMedia does too — but a plain-http dev server would otherwise
  // fail here rather than at the camera, which is a confusing place to
  // learn about it.
  if (globalThis.crypto?.randomUUID) return crypto.randomUUID();
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

// Ask the browser not to evict this origin's data under storage pressure.
//
// Returns whether the library is durable. Chrome grants this silently to an
// installed PWA; Safari and Firefox have their own rules. The answer is
// surfaced in the UI rather than swallowed, because "your documents may be
// cleared if the browser needs space" is something a user scanning tax
// records deserves to be told before they find out.
export async function requestPersistence() {
  if (!navigator.storage?.persist) return false;
  try {
    if (await navigator.storage.persisted()) return true;
    return await navigator.storage.persist();
  } catch {
    return false;
  }
}

export async function storageEstimate() {
  if (!navigator.storage?.estimate) return null;
  try {
    return await navigator.storage.estimate();
  } catch {
    return null;
  }
}

// Write a document and all of its pages.
//
// One transaction covering both stores, so a crash or a force-quit
// mid-save leaves either the whole document or none of it. A document row
// pointing at pages that were never written is the exact "silent document
// loss" PLAN.md's success metrics forbid, and the only way to rule it out
// is for the two writes never to be separately abortable.
export async function saveDocument(doc, pages) {
  const db = await openDb();
  const { transaction, done } = tx(db, ['documents', 'pages'], 'readwrite');
  const documents = transaction.objectStore('documents');
  const pageStore = transaction.objectStore('pages');

  // Replacing an existing document: clear its old pages first, or a
  // re-save with fewer pages leaves orphans behind.
  const existing = await request(pageStore.index('docId').getAllKeys(doc.id));
  for (const key of existing) pageStore.delete(key);

  documents.put(doc);
  pages.forEach((page, index) => {
    pageStore.put({ ...page, docId: doc.id, index });
  });

  await done;
  return doc.id;
}

export async function listDocuments() {
  const db = await openDb();
  const all = await request(db.transaction('documents').objectStore('documents').getAll());
  return all.sort((a, b) => b.updated - a.updated);
}

export async function getDocument(id) {
  const db = await openDb();
  return request(db.transaction('documents').objectStore('documents').get(id));
}

export async function getPages(docId) {
  const db = await openDb();
  const pages = await request(
    db.transaction('pages').objectStore('pages').index('docId').getAll(docId),
  );
  return pages.sort((a, b) => a.index - b.index);
}

export async function deleteDocument(id) {
  const db = await openDb();
  const { transaction, done } = tx(db, ['documents', 'pages'], 'readwrite');
  transaction.objectStore('documents').delete(id);
  const pageStore = transaction.objectStore('pages');
  const keys = await request(pageStore.index('docId').getAllKeys(id));
  for (const key of keys) pageStore.delete(key);
  await done;
}

// Documents whose title or recognised text contains every word of the query.
//
// Every word rather than the whole phrase: OCR reads a page as a sequence
// of words and this index joins them with single spaces, so the original
// line breaks and column gaps are gone. Searching for the literal phrase
// "invoice total" would then miss a page where those two words sat in
// different columns — which is most invoices.
export async function searchDocuments(query) {
  const documents = await listDocuments();
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return documents;

  return documents.filter((doc) => {
    const haystack = `${doc.title ?? ''} ${doc.text ?? ''}`.toLowerCase();
    return terms.every((term) => haystack.includes(term));
  });
}

export async function getSetting(key, fallback) {
  const db = await openDb();
  const row = await request(db.transaction('settings').objectStore('settings').get(key));
  return row === undefined ? fallback : row.value;
}

export async function setSetting(key, value) {
  const db = await openDb();
  const { transaction, done } = tx(db, ['settings'], 'readwrite');
  transaction.objectStore('settings').put({ key, value });
  await done;
}

// Check that every document still has the pages it claims to.
//
// Runs at startup. A document whose pages went missing is reported rather
// than hidden or deleted: the user gets told which document is damaged and
// keeps whatever pages survived, which is strictly better than a library
// that quietly shows one fewer item than it did yesterday.
export async function checkIntegrity() {
  const documents = await listDocuments();
  const damaged = [];

  for (const doc of documents) {
    const pages = await getPages(doc.id);
    if (pages.length !== doc.pageCount) {
      damaged.push({
        id: doc.id,
        title: doc.title,
        expected: doc.pageCount,
        found: pages.length,
      });
    }
  }

  return damaged;
}
