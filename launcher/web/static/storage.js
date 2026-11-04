// The game's files (saves, autosaves, journals, Hall of Fame, settings) in IndexedDB.
// The game keeps every file in memory (platform/crates/client/src/shell/storage) and calls two things here:
//   aureliaStorage.load()                     every stored file, once at start: [[key, Uint8Array, modifiedMs], ...]
//   aureliaStorage.commit(keys, bytes, times) one all-or-nothing transaction; bytes[i] === null deletes keys[i].
//                                             Resolves true when the database had to be opened anew since the
//                                             last call (it was deleted behind the page): the game then writes
//                                             everything again.
// and hands over one function: aureliaStorage.setFlushHook(fn), which the page calls when it is hidden or
// closed, so the game starts its last write before the page goes. localStorage is not used: saves are megabytes.
// Several tabs: each tab holds its own copy from the time it started and writes what it changed; the last
// write of a file wins.
(function () {
  "use strict";

  const NAME = "aurelia", STORE = "files", VERSION = 1;
  let opening = null;   // Promise<IDBDatabase>, while the connection is good
  let reopened = false; // the connection was lost and made anew since the last commit/load

  function open() {
    if (opening) return opening;
    if (typeof indexedDB === "undefined" || !indexedDB) {
      return Promise.reject(new Error("IndexedDB is not available in this browser"));
    }
    const lost = () => { opening = null; reopened = true; };
    const attempt = new Promise((resolve, reject) => {
      let request;
      try { request = indexedDB.open(NAME, VERSION); } catch (error) { reject(error); return; }
      request.onupgradeneeded = () => {
        if (!request.result.objectStoreNames.contains(STORE)) request.result.createObjectStore(STORE);
      };
      request.onsuccess = () => {
        const db = request.result;
        // Deleted from the developer tools or upgraded by another tab: let it go, open anew when next needed.
        db.onversionchange = () => { db.close(); lost(); };
        db.onclose = lost;
        resolve(db);
      };
      request.onerror = () => reject(request.error || new Error("IndexedDB could not be opened"));
      request.onblocked = () => reject(new Error("IndexedDB is blocked by another tab"));
    });
    opening = attempt;
    attempt.catch(() => { if (opening === attempt) opening = null; });
    return attempt;
  }

  function load() {
    reopened = false;
    return open().then((db) => new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, "readonly");
      const keys = tx.objectStore(STORE).getAllKeys();
      const values = tx.objectStore(STORE).getAll();
      tx.oncomplete = () => {
        const out = [];
        keys.result.forEach((key, i) => {
          const value = values.result[i];
          if (typeof key === "string" && value && value.bytes instanceof Uint8Array) {
            out.push([key, value.bytes, Number(value.modified) || 0]);
          }
        });
        resolve(out);
      };
      tx.onerror = tx.onabort = () => reject(tx.error || new Error("IndexedDB read failed"));
    }));
  }

  function commit(keys, bytes, times) {
    return open().then((db) => new Promise((resolve, reject) => {
      const fresh = reopened;
      let tx;
      try {
        tx = db.transaction(STORE, "readwrite");
        const store = tx.objectStore(STORE);
        keys.forEach((key, i) => {
          if (bytes[i] === null) store.delete(key);
          else store.put({ bytes: bytes[i], modified: times[i] }, key);
        });
      } catch (error) {
        if (tx) { try { tx.abort(); } catch (ignored) { /* already over */ } }
        reject(error);
        return;
      }
      tx.oncomplete = () => { reopened = false; resolve(fresh); };
      tx.onerror = tx.onabort = () => reject(tx.error || new Error("IndexedDB write failed"));
    }));
  }

  let hook = null;
  function hide() { if (hook) { try { hook(); } catch (error) { console.error(error); } } }
  addEventListener("pagehide", hide);
  document.addEventListener("visibilitychange", () => { if (document.visibilityState === "hidden") hide(); });

  window.aureliaStorage = { load, commit, setFlushHook: (fn) => { hook = fn; } };
})();
