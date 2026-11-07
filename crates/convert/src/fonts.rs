//! Process-level font registry (convert spec §6.1 D9/D19): the single
//! door for font injection into headless conversion. Registration is
//! process-global, append-only (no unregister — a registry that could
//! shrink under per-document uploads would break the "same process font
//! state → same output bytes" determinism contract) and thread-safe.
//!
//! Validation uses a scratch [`FontStore`] probe: bytes that do not parse
//! as a font are rejected with
//! [`FontRegisterError::InvalidFont`](crate::FontRegisterError::InvalidFont)
//! and never enter the registry. Successful registrations also record
//! their family names (via the probe's
//! [`registered_family_names`](rofd_render::FontStore::registered_family_names))
//! for CLI feedback.

use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use crate::FontRegisterError;
use rofd_dom::Resources;
use rofd_render::FontStore;

/// Immutable view of the registry for a [`Converter`](crate::Converter)
/// snapshot. `Arc`-cloned from storage — zero font-byte copies. Consumed
/// by [`Converter::new`](crate::Converter::new).
pub(crate) struct RegistrySnapshot {
    pub fonts: Vec<Arc<Vec<u8>>>,
    pub default_font: Option<Arc<Vec<u8>>>,
}

/// Registry storage: font bytes (Arc-shared), recorded family names, the
/// default-font anchor (D19) and the fingerprints of every successfully
/// registered payload.
struct Registry {
    fonts: Vec<Arc<Vec<u8>>>,
    families: Vec<String>,
    default_font: Option<Arc<Vec<u8>>>,
    fingerprints: HashSet<(usize, u64)>,
}

/// The process-level registry. A `Mutex` (not `RwLock`) mirrors rword's
/// fonts.rs: only `Send` is needed for `Sync`, and a poisoned lock is
/// recovered with `into_inner` — a conversion process must not be
/// permanently disabled by one panic while the lock was held.
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            fonts: Vec::new(),
            families: Vec::new(),
            default_font: None,
            fingerprints: HashSet::new(),
        })
    })
}

/// Lock the registry, recovering from poisoning (see [`REGISTRY`]).
fn lock_registry() -> MutexGuard<'static, Registry> {
    registry()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// (len, content hash) fingerprint of a payload. std's `DefaultHasher` is
/// fixed-seed and stable within a process — cross-process stability is
/// not required (the set is only consulted against bytes seen in THIS
/// process); collision safety comes from pairing the hash with the exact
/// length. Mirrors rword's rword-export fonts.rs.
fn fingerprint(bytes: &[u8]) -> (usize, u64) {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    (bytes.len(), h.finish())
}

/// Validate `bytes` with a scratch [`FontStore`] probe and return the
/// family names a successful registration would surface. `None` = the
/// bytes are not a usable font.
fn probe_family_names(bytes: &[u8]) -> Option<Vec<String>> {
    let mut store = FontStore::from_resources(&Resources::default(), Arc::new(vec![]));
    if !store.register_font(Arc::new(bytes.to_vec())) {
        return None;
    }
    Some(store.registered_family_names())
}

/// Shared body of [`register_font`] / [`register_default_font`]: dedup on
/// a known fingerprint (idempotent Ok, no re-probe), otherwise validate
/// via the probe and record bytes + families. Returns the `Arc` the bytes
/// live under — a fresh `Arc` even for a fingerprint duplicate (the
/// default anchor may alias equal bytes without sharing the allocation;
/// one font's worth of duplicate memory is the accepted cost of last-wins
/// D19).
fn register_validated(
    reg: &mut Registry,
    bytes: Vec<u8>,
) -> Result<Arc<Vec<u8>>, FontRegisterError> {
    let fp = fingerprint(&bytes);
    if reg.fingerprints.contains(&fp) {
        // Known-good bytes: duplicate faces perturb unrelated fallback
        // picks (rword finding), so registration is a no-op.
        return Ok(Arc::new(bytes));
    }
    let names = probe_family_names(&bytes).ok_or(FontRegisterError::InvalidFont)?;
    let arc = Arc::new(bytes);
    reg.fingerprints.insert(fp);
    reg.fonts.push(arc.clone());
    for name in names {
        if !reg.families.contains(&name) {
            reg.families.push(name);
        }
    }
    Ok(arc)
}

/// Register font bytes into the process-level registry (thread-safe,
/// append-only). Idempotent on identical byte content: re-registering
/// bytes already in the registry is an `Ok(())` no-op (duplicate faces
/// measurably perturb fallback ordering for unrelated families).
/// Unparsable bytes yield
/// [`FontRegisterError::InvalidFont`](crate::FontRegisterError::InvalidFont).
pub fn register_font(bytes: Vec<u8>) -> Result<(), FontRegisterError> {
    let mut reg = lock_registry();
    register_validated(&mut reg, bytes).map(|_| ())
}

/// Read a font file and register it into the process-level registry. A
/// read failure is returned as
/// [`FontRegisterError::Io`](crate::FontRegisterError::Io); the parsed
/// bytes follow [`register_font`] semantics.
pub fn register_font_file(path: impl AsRef<Path>) -> Result<(), FontRegisterError> {
    let bytes = std::fs::read(path).map_err(FontRegisterError::Io)?;
    register_font(bytes)
}

/// Register font bytes AND anchor them as the default font (spec D19):
/// the fallback for body text whose declared family has no registered
/// bytes. Last registration wins. Validation and fingerprint dedup
/// follow [`register_font`]; the anchor is set even when the bytes were
/// already registered.
pub fn register_default_font(bytes: Vec<u8>) -> Result<(), FontRegisterError> {
    let mut reg = lock_registry();
    let arc = register_validated(&mut reg, bytes)?;
    reg.default_font = Some(arc);
    Ok(())
}

/// Font families recorded by successful process-level registrations, in
/// registration order. System-discovered families are never listed.
pub fn registered_families() -> Vec<String> {
    lock_registry().families.clone()
}

/// Whether a default font is anchored (CLI feedback for "no default font
/// registered — output may fall back to system fonts").
pub fn has_default_font() -> bool {
    lock_registry().default_font.is_some()
}

/// Snapshot the registry for a [`Converter`](crate::Converter): fonts
/// registered before this call are visible to every convert of the new
/// instance; later registrations need a new instance (spec D9).
/// `Arc`-clones only — zero font-byte copies.
pub(crate) fn registry_snapshot() -> RegistrySnapshot {
    let reg = lock_registry();
    RegistrySnapshot {
        fonts: reg.fonts.clone(),
        default_font: reg.default_font.clone(),
    }
}
