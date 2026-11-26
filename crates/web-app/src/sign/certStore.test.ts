import { describe, expect, it, vi } from 'vitest';
import type { CertificateEntry } from '@office-rs/rofd';
import { WebCertStore, indexedDbStorage, type CertStorage } from './certStore';

const PFX = new Uint8Array([1, 2, 3, 4]);

function entryFor(id: string): CertificateEntry {
  return {
    id,
    label: `证书 ${id}`,
    subject_cn: `证书 ${id}`,
    issuer_cn: '测试 CA',
    serial: '01719D2DAB60',
    not_before: '20200101000000Z',
    not_after: '20991231235959Z',
  };
}

/** A storage that keeps its bytes in memory (what IndexedDB would hold). */
function memoryStorage(): CertStorage & { bytes: Uint8Array | null } {
  const storage = {
    bytes: null as Uint8Array | null,
    load: vi.fn(async (): Promise<Uint8Array | null> => storage.bytes),
    save: vi.fn(async (bytes: Uint8Array): Promise<void> => {
      storage.bytes = bytes.slice();
    }),
  };
  return storage;
}

const WRONG_PASSWORD = "certificate: wrong password (the file's integrity check failed)";

/** The SDK's certificateInfo, as a test double: id `a` opens with
 * `secret`, anything else is a wrong password. */
const readEntry = vi.fn(async (file: Uint8Array, password: string): Promise<CertificateEntry> => {
  if (file[0] === 1 && password === 'secret') {
    return entryFor('a');
  }
  if (file[0] === 9) {
    return entryFor('b');
  }
  throw new Error(WRONG_PASSWORD);
});

describe('WebCertStore', () => {
  it('persists through the storage and lists imported certificates', async () => {
    const storage = memoryStorage();
    const store = new WebCertStore(storage, readEntry);
    expect(await store.list()).toEqual([]);

    const entry = await store.import(PFX, 'secret');
    expect(entry).toEqual(entryFor('a'));
    expect(await store.list()).toEqual([entryFor('a')]);
    // The certificate file is kept as imported, for signing with the
    // password the user gives then.
    expect(await store.signerBytes('a')).toEqual(PFX);
    expect(await store.signerBytes('no-such-id')).toBeNull();
    expect(storage.save).toHaveBeenCalledTimes(1);

    // Another store over the same storage sees it (a new session).
    const again = new WebCertStore(storage, readEntry);
    expect(await again.list()).toEqual([entryFor('a')]);
    expect(await again.signerBytes('a')).toEqual(PFX);

    // A wrong password adds nothing.
    await expect(store.import(new Uint8Array([7]), 'wrong')).rejects.toThrow(
      /wrong password/,
    );
    expect(await store.list()).toEqual([entryFor('a')]);
    expect(storage.save).toHaveBeenCalledTimes(1);

    // Importing another certificate adds it; the same one again replaces.
    await store.import(new Uint8Array([9, 9]), 'x');
    expect((await store.list()).map((e) => e.id).sort()).toEqual(['a', 'b']);
    await store.import(new Uint8Array([1, 2, 3, 5]), 'secret');
    expect((await store.list()).map((e) => e.id).sort()).toEqual(['a', 'b']);
    // ... and the newer file is the one kept.
    expect(await store.signerBytes('a')).toEqual(new Uint8Array([1, 2, 3, 5]));

    await store.remove('a');
    expect(await store.list()).toEqual([entryFor('b')]);
    expect(await store.signerBytes('a')).toBeNull();
    // Removing what is not there changes nothing.
    await store.remove('a');
    expect(await store.list()).toEqual([entryFor('b')]);
  });

  it('fails loudly on a storage that cannot be read', async () => {
    const broken: CertStorage = {
      load: () => Promise.reject(new Error('quota exceeded')),
      save: () => Promise.resolve(),
    };
    const store = new WebCertStore(broken, readEntry);
    await expect(store.list()).rejects.toThrow(/quota exceeded/);
  });

  it('refuses bytes that are not a certificate store', async () => {
    const storage = memoryStorage();
    storage.bytes = new TextEncoder().encode('not a store');
    const store = new WebCertStore(storage, readEntry);
    await expect(store.list()).rejects.toThrow();
  });

  it('indexedDbStorage is a single-blob storage', async () => {
    const storage = indexedDbStorage();
    expect(typeof storage.load).toBe('function');
    expect(typeof storage.save).toBe('function');
    // Without IndexedDB (happy-dom) it says so instead of losing data.
    if (!('indexedDB' in globalThis)) {
      await expect(storage.load()).rejects.toThrow(/IndexedDB/);
    } else {
      expect(await storage.load()).toBeNull();
      await storage.save(new Uint8Array([1]));
      expect(await storage.load()).toEqual(new Uint8Array([1]));
    }
  });
});
