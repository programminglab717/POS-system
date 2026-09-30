//! Known-answer tests for encryption at rest: what the store's files hold, which keys open them,
//! and changing the key (ADR-0018).

use super::*;

/// Whether `needle` appears anywhere in `haystack`.
fn holds(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// The WAL journal beside the database at `path`.
fn journal_of(path: &Path) -> PathBuf {
    let mut journal = path.as_os_str().to_owned();
    journal.push("-wal");
    PathBuf::from(journal)
}

#[test]
fn neither_the_database_nor_its_journal_holds_anything_in_the_clear() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let events = append(&mut store, vec![draft(1, 1), draft(2, 2), draft(1, 3)], at(0));
    // What an event holds in the clear: its identifier, its bytes, and its schema's name; and
    // what any SQLite database holds: its header and its schema.
    let mut clear: Vec<Vec<u8>> = events
        .iter()
        .flat_map(|event| [event.body().event_id.to_bytes().to_vec(), event.to_bytes()])
        .collect();
    for text in ["order.noted", "SQLite format 3", "CREATE TABLE", "quarantine", "orders"] {
        clear.push(text.as_bytes().to_vec());
    }
    // While the store is open, the pages it wrote are in the journal.
    let journal = std::fs::read(journal_of(&dir.db())).unwrap();
    assert!(journal.len() > 4096, "the journal holds pages");
    for text in &clear {
        assert!(!holds(&journal, text), "the journal holds {text:?}");
    }
    drop(store);
    // Closed, the store is all in the database file.
    assert!(!journal_of(&dir.db()).exists());
    let file = std::fs::read(dir.db()).unwrap();
    assert!(file.len() > 4096);
    for text in &clear {
        assert!(!holds(&file, text), "the database holds {text:?}");
    }
}

#[test]
fn a_store_opens_only_with_its_key() {
    let dir = TempDir::new();
    let events = append(&mut open(&dir.db()), vec![draft(1, 1)], at(0));
    let other = Store::open(dir.db(), key_of(0x4C), config(), own().1, SeededEntropy::new(1));
    assert!(matches!(other, Err(StoreError::KeyRejected)));
    // Refused, it changed nothing.
    assert_eq!(open(&dir.db()).log(own().0, 0, 10).unwrap(), events);
    // An unencrypted database, and a file that isn't a database, don't open either.
    let plain = TempDir::new();
    let db = Connection::open(plain.db()).unwrap();
    db.execute_batch("CREATE TABLE t (x); INSERT INTO t VALUES (1);").unwrap();
    drop(db);
    let garbage = TempDir::new();
    std::fs::write(garbage.db(), [0x5A; 8192]).unwrap();
    for path in [plain.db(), garbage.db()] {
        let opened = Store::open(&path, key(), config(), own().1, SeededEntropy::new(1));
        assert!(matches!(opened, Err(StoreError::KeyRejected)), "{path:?}");
    }
}

#[test]
fn the_cipher_and_its_settings_are_pinned() {
    let dir = TempDir::new();
    let store = open(&dir.db());
    let text = |sql: &str| -> Vec<String> {
        let mut statement = store.db().prepare(sql).unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    };
    // SQLCipher 4's settings, which the golden store pins on disk.
    assert_eq!(
        text("PRAGMA cipher_settings"),
        [
            "PRAGMA kdf_iter = 256000;",
            "PRAGMA cipher_page_size = 4096;",
            "PRAGMA cipher_use_hmac = 1;",
            "PRAGMA cipher_plaintext_header_size = 0;",
            "PRAGMA cipher_hmac_algorithm = HMAC_SHA512;",
            "PRAGMA cipher_kdf_algorithm = PBKDF2_HMAC_SHA512;",
        ]
    );
    assert_eq!(text("PRAGMA cipher_provider"), ["openssl"]);
    // The store reports what goes wrong itself: SQLCipher writes nothing to the process's output.
    assert_eq!(text("PRAGMA cipher_log_level"), ["NONE"]);
    assert!(text("PRAGMA cipher_version")[0].starts_with("4."));
    // Temporary tables and indexes never go to a file.
    let temp_store: i64 = store.db().query_row("PRAGMA temp_store", [], |row| row.get(0)).unwrap();
    assert_eq!(temp_store, 2);
}

#[test]
fn a_rekeyed_store_opens_only_with_its_new_key() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let before = append(&mut store, vec![draft(1, 1), draft(1, 2)], at(0));
    store.rekey(key_of(0x4C)).unwrap();
    // The store goes on, and its journal was moved into the database under the new key.
    let after = append(&mut store, vec![draft(1, 3)], at(1));
    assert_eq!(store.check().unwrap(), []);
    drop(store);
    let old = Store::open(dir.db(), key(), config(), own().1, SeededEntropy::new(1));
    assert!(matches!(old, Err(StoreError::KeyRejected)));
    let store = Store::open(dir.db(), key_of(0x4C), config(), own().1, SeededEntropy::new(1));
    let mut store = store.unwrap();
    assert_eq!(store.log(own().0, 0, 10).unwrap(), [before, after].concat());
    assert_eq!(store.check().unwrap(), []);
    // No page authenticates under the old key.
    drop(store);
    let db = raw(&dir.db());
    let failed: Vec<String> = {
        let mut statement = db.prepare("PRAGMA cipher_integrity_check").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    };
    let pages = std::fs::metadata(dir.db()).unwrap().len() / 4096;
    assert_eq!(u64::try_from(failed.len()).unwrap(), pages, "{failed:?}");
}

/// Found in designing the store: SQLCipher 4.14's rekey reports success when it can't rewrite
/// the store, as when another connection holds its write lock, and its connection would go on
/// writing pages under the new key, leaving a store neither key opens.
#[test]
fn a_rekey_sqlcipher_cant_do_is_reported_and_changes_nothing() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    let before = append(&mut store, vec![draft(1, 1)], at(0));
    // A rekey that works, then one that can't: the store goes back to its key since the first.
    store.rekey(key_of(0x4C)).unwrap();
    let other = raw_with(&dir.db(), 0x4C);
    other.execute_batch("BEGIN IMMEDIATE").unwrap();
    // SQLite waits out its busy timeout, five seconds, first.
    assert!(matches!(store.rekey(key_of(0x4D)), Err(StoreError::NotRekeyed)));
    other.execute_batch("ROLLBACK").unwrap();
    drop(other);
    // The store goes on under that key.
    let after = append(&mut store, vec![draft(1, 2), draft(1, 3)], at(1));
    assert_eq!(store.check().unwrap(), []);
    drop(store);
    for refused in [key(), key_of(0x4D)] {
        let opened = Store::open(dir.db(), refused, config(), own().1, SeededEntropy::new(1));
        assert!(matches!(opened, Err(StoreError::KeyRejected)));
    }
    let store = Store::open(dir.db(), key_of(0x4C), config(), own().1, SeededEntropy::new(2));
    let mut store = store.unwrap();
    assert_eq!(store.log(own().0, 0, 10).unwrap(), [before, after].concat());
    assert_eq!(store.check().unwrap(), []);
}

#[test]
fn an_interrupted_rekey_changes_nothing() {
    let dir = TempDir::new();
    let faults = Box::new(RefuseAt { point: Point::Rekeying, nth: 1, seen: 0 });
    let mut store =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(7), faults)
            .unwrap();
    let events = append(&mut store, vec![draft(1, 1)], at(0));
    assert!(matches!(store.rekey(key_of(0x4C)), Err(StoreError::Interrupted(Point::Rekeying))));
    drop(store);
    assert_eq!(open(&dir.db()).log(own().0, 0, 10).unwrap(), events);
    // Refusing once the rekey has happened changes nothing either: the new key is the store's.
    let faults = Box::new(RefuseAt { point: Point::Rekeyed, nth: 1, seen: 0 });
    let mut store =
        Store::open_with_faults(dir.db(), key(), config(), own().1, SeededEntropy::new(8), faults)
            .unwrap();
    store.rekey(key_of(0x4C)).unwrap();
    drop(store);
    let store = Store::open(dir.db(), key_of(0x4C), config(), own().1, SeededEntropy::new(9));
    assert_eq!(store.unwrap().log(own().0, 0, 10).unwrap(), events);
}

#[test]
fn a_journal_left_behind_says_the_store_wasnt_closed_cleanly() {
    let dir = TempDir::new();
    let mut store = open(&dir.db());
    assert!(!store.recovered(), "a new store");
    let events = append(&mut store, vec![draft(1, 1), draft(1, 2)], at(0));
    // A crash, as the files are while the store is open.
    let crashed = TempDir::new();
    std::fs::copy(dir.db(), crashed.db()).unwrap();
    std::fs::copy(journal_of(&dir.db()), journal_of(&crashed.db())).unwrap();
    drop(store);
    assert!(!open(&dir.db()).recovered(), "a store closed cleanly");
    // Trying another key first, as a shell does after a crash in a rotation, leaves the journal.
    let other = Store::open(crashed.db(), key_of(0x4C), config(), own().1, SeededEntropy::new(1));
    assert!(matches!(other, Err(StoreError::KeyRejected)));
    let mut store = open(&crashed.db());
    assert!(store.recovered());
    assert_eq!(store.log(own().0, 0, 10).unwrap(), events);
    assert_eq!(store.check().unwrap(), []);
    drop(store);
    assert!(!open(&crashed.db()).recovered(), "closed cleanly since");
}

#[test]
fn a_key_zeroes_what_it_was_made_from_and_never_prints() {
    let mut bytes = [0x5A; 32];
    let key = StoreKey::new(&mut bytes);
    assert_eq!(bytes, [0; 32]);
    assert_eq!(format!("{key:?}"), "StoreKey(..)");
}
