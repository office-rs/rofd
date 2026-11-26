// The web host's certificate store (spec 2026-11-23 §10.5): the
// certificates a user imported, kept in IndexedDB as one blob.
//
// The blob is the same JSON rofd-sign's `CertificateStore::to_bytes`
// writes on the native hosts - an array of display data plus each
// certificate file, byte for byte as imported and still encrypted, as
// base64 - so a store can be moved between hosts. Nothing here holds a
// password or key material: importing opens the file once with its
// password to read what to show for it, and signing asks for the password
// again (M7).

import { certificateInfo, type CertificateEntry } from '@office-rs/rofd';

/** Where the store's blob lives. One blob: the whole store is rewritten
 * after each change. */
export interface CertStorage {
  /** The blob, or `null` when nothing was stored yet. */
  load(): Promise<Uint8Array | null>;
  save(bytes: Uint8Array): Promise<void>;
}

/** Reads a certificate file with a password (the SDK's `certificateInfo`):
 * resolves to what to show for it, rejects on a wrong password. */
export type ReadCertificateEntry = (
  file: Uint8Array,
  password: string,
) => Promise<CertificateEntry>;

/** One certificate as the blob holds it. */
interface StoredCertificate {
  entry: CertificateEntry;
  /** The certificate file exactly as imported, still encrypted. */
  file: Uint8Array;
}

function bytesToBase64(bytes: Uint8Array): string {
  let text = '';
  for (let at = 0; at < bytes.length; at += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  }
  return btoa(text);
}

function base64ToBytes(text: string): Uint8Array {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** The blob: JSON in rofd-sign's store shape (snake_case, `file` base64). */
function encodeStore(certificates: StoredCertificate[]): Uint8Array {
  const json = JSON.stringify(
    certificates.map(({ entry, file }) => ({
      id: entry.id,
      label: entry.label,
      subject_cn: entry.subject_cn,
      issuer_cn: entry.issuer_cn,
      serial: entry.serial,
      not_before: entry.not_before,
      not_after: entry.not_after,
      file: bytesToBase64(file),
    })),
  );
  return new TextEncoder().encode(json);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null;
}

/** Parse a blob. Anything that is not a store is an error: silently
 * starting over would lose the user's certificates. */
function decodeStore(bytes: Uint8Array): StoredCertificate[] {
  const fail = (): never => {
    throw new Error('证书库数据无法读取');
  };
  let json: unknown;
  try {
    json = JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    return fail();
  }
  if (!Array.isArray(json)) {
    return fail();
  }
  return json.map((item: unknown) => {
    if (
      !isRecord(item) ||
      typeof item.id !== 'string' ||
      typeof item.file !== 'string'
    ) {
      return fail();
    }
    const text = (key: string): string | null =>
      typeof item[key] === 'string' ? (item[key] as string) : null;
    return {
      entry: {
        id: item.id,
        label: text('label') ?? '证书',
        subject_cn: text('subject_cn') ?? '',
        issuer_cn: text('issuer_cn') ?? '',
        serial: text('serial') ?? '',
        not_before: text('not_before'),
        not_after: text('not_after'),
      },
      file: base64ToBytes(item.file),
    };
  });
}

/** The certificates a user imported, persisted through a
 * [`CertStorage`]. The blob is read on first use and rewritten after
 * every change. */
export class WebCertStore {
  readonly #storage: CertStorage;
  readonly #readEntry: ReadCertificateEntry;
  /** `null` until the blob was read. */
  #certificates: StoredCertificate[] | null = null;

  constructor(storage: CertStorage, readEntry: ReadCertificateEntry = certificateInfo) {
    this.#storage = storage;
    this.#readEntry = readEntry;
  }

  async #load(): Promise<StoredCertificate[]> {
    if (this.#certificates === null) {
      const bytes = await this.#storage.load();
      this.#certificates = bytes === null ? [] : decodeStore(bytes);
    }
    return this.#certificates;
  }

  async #store(certificates: StoredCertificate[]): Promise<void> {
    await this.#storage.save(encodeStore(certificates));
    this.#certificates = certificates;
  }

  /** The imported certificates, by label. */
  async list(): Promise<CertificateEntry[]> {
    const certificates = await this.#load();
    return certificates.map(({ entry }) => ({ ...entry }));
  }

  /** Import a certificate file, opening it once with `password` to read
   * what to show for it. Importing the same certificate again replaces
   * its entry. Rejects on a wrong password; nothing is added then. */
  async import(file: Uint8Array, password: string): Promise<CertificateEntry> {
    const entry = await this.#readEntry(file, password);
    const certificates = await this.#load();
    const kept = certificates.filter(({ entry: kept }) => kept.id !== entry.id);
    const next = [...kept, { entry, file: file.slice() }].sort((a, b) =>
      a.entry.label.toLowerCase().localeCompare(b.entry.label.toLowerCase()),
    );
    await this.#store(next);
    return { ...entry };
  }

  /** Forget a certificate. */
  async remove(id: string): Promise<void> {
    const certificates = await this.#load();
    const next = certificates.filter(({ entry }) => entry.id !== id);
    if (next.length !== certificates.length) {
      await this.#store(next);
    }
  }

  /** An imported certificate's file, for signing with
   * `signOfd(bytes, file, password, options)`; `null` for an unknown id. */
  async signerBytes(id: string): Promise<Uint8Array | null> {
    const certificates = await this.#load();
    const found = certificates.find(({ entry }) => entry.id === id);
    return found === undefined ? null : found.file.slice();
  }
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (!('indexedDB' in globalThis)) {
      reject(new Error('当前浏览器不支持 IndexedDB，证书无法保存'));
      return;
    }
    const request = indexedDB.open('rofd', 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore('files');
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () =>
      reject(request.error ?? new Error('无法打开 IndexedDB（rofd）'));
    request.onblocked = () => reject(new Error('IndexedDB（rofd）被其他窗口占用'));
  });
}

/** The store's blob in IndexedDB: database `rofd`, store `files`, one key. */
export function indexedDbStorage(): CertStorage {
  const KEY = 'certificates';
  const read = async (): Promise<Uint8Array | null> => {
    const db = await openDatabase();
    try {
      return await new Promise<Uint8Array | null>((resolve, reject) => {
        const request = db.transaction('files', 'readonly').objectStore('files').get(KEY);
        request.onsuccess = () => {
          const value: unknown = request.result;
          if (value instanceof Uint8Array) {
            resolve(value);
          } else if (value === undefined) {
            resolve(null);
          } else {
            reject(new Error('证书库数据无法读取'));
          }
        };
        request.onerror = () =>
          reject(request.error ?? new Error('无法读取 IndexedDB（rofd）'));
      });
    } finally {
      db.close();
    }
  };
  const write = async (bytes: Uint8Array): Promise<void> => {
    const db = await openDatabase();
    try {
      await new Promise<void>((resolve, reject) => {
        const transaction = db.transaction('files', 'readwrite');
        transaction.objectStore('files').put(bytes.slice(), KEY);
        transaction.oncomplete = () => resolve();
        transaction.onerror = () =>
          reject(transaction.error ?? new Error('无法写入 IndexedDB（rofd）'));
        transaction.onabort = () =>
          reject(transaction.error ?? new Error('无法写入 IndexedDB（rofd）'));
      });
    } finally {
      db.close();
    }
  };
  return { load: read, save: write };
}
