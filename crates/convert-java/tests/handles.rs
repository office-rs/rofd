//! Handle-table integration tests — pure Rust, no JVM involved. The table
//! turns the D15 "one thread at a time per instance" contract into
//! mechanics: take() removes the instance for the duration of a call,
//! put() returns it; a concurrent second take of the same id reports
//! InUse.
//!
//! Note the import name: the package is `rofd-convert-java` but its
//! `[lib] name` is `rofdconvert` (the JVM-facing library name), so the
//! rlib is imported as `rofdconvert`.

use rofdconvert::handles::{HandleTable, TakeError};

#[test]
fn lifecycle_insert_take_put_remove() {
    let t = HandleTable::new();
    let id = t.insert(rofd_convert::Converter::new());
    // two take->put round trips in a row
    for _ in 0..2 {
        let c = t.take(id).expect("inserted id must take");
        t.put(id, c);
    }
    // close = remove + drop
    assert!(t.close(id).is_ok());
}

#[test]
fn double_take_is_in_use_and_unknown_id_is_stale() {
    let t = HandleTable::new();
    let id = t.insert(rofd_convert::Converter::new());
    let _leased = t.take(id).unwrap();
    assert!(matches!(t.take(id), Err(TakeError::InUse)));
    t.put(id, _leased);
    t.close(id).unwrap();
    // ids are never reused: a stale close/take after close is Stale, not a hit
    assert!(matches!(t.take(id), Err(TakeError::Stale)));
    assert!(matches!(t.close(id), Err(TakeError::Stale)));
}

#[test]
fn ids_never_collide_across_close_cycles() {
    let t = HandleTable::new();
    let a = t.insert(rofd_convert::Converter::new());
    t.close(a).unwrap();
    let b = t.insert(rofd_convert::Converter::new());
    assert_ne!(a, b, "monotonic ids prevent ABA on stale Java handles");
}

#[test]
fn put_into_closed_slot_is_a_noop_not_a_resurrection() {
    let t = HandleTable::new();
    let id = t.insert(rofd_convert::Converter::new());
    let leased = t.take(id).unwrap();
    // close() races a leased converter: the map entry is removed now; the
    // lease's put() must not re-insert the instance under the stale id.
    t.close(id).unwrap();
    t.put(id, leased);
    assert!(matches!(t.take(id), Err(TakeError::Stale)));
}
