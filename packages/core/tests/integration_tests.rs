use l10n4x_core::binary_format::BinaryFormatReader;

use l10n4x_core::formatter::{format_message, PluralCategory};
use l10n4x_core::plural_rules::get_plural_category;
#[cfg(feature = "std")]
use l10n4x_core::store::read_store;
use l10n4x_core::store::{swap_store, translate, TranslationStore};
use std::sync::Arc;
#[cfg(feature = "std")]
use std::thread;

#[test]
fn test_binary_format_reader_mock() {
    // Construct a mock binary format buffer manually
    // Keys: "a", "b"
    // Values: "val_a", "val_b"
    let mut data = Vec::new();
    data.extend_from_slice(b"L10N");
    data.extend_from_slice(&1u32.to_be_bytes()); // version
    data.extend_from_slice(&0u32.to_be_bytes()); // index offset (fill later)
    data.extend_from_slice(&2u32.to_be_bytes()); // index count

    // data starts at 16
    // key "a" offset 16, len 1
    data.extend_from_slice(b"a");
    // val "val_a" offset 17, len 5
    data.extend_from_slice(b"val_a");
    // key "b" offset 22, len 1
    data.extend_from_slice(b"b");
    // val "val_b" offset 23, len 5
    data.extend_from_slice(b"val_b");

    let index_offset = data.len();
    // entry 1: key "a" (16, 1), val "val_a" (17, 5)
    data.extend_from_slice(&16u32.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&17u32.to_be_bytes());
    data.extend_from_slice(&5u32.to_be_bytes());

    // entry 2: key "b" (22, 1), val "val_b" (23, 5)
    data.extend_from_slice(&22u32.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());
    data.extend_from_slice(&23u32.to_be_bytes());
    data.extend_from_slice(&5u32.to_be_bytes());

    // update index offset in header
    data[8..12].copy_from_slice(&(index_offset as u32).to_be_bytes());

    let reader = BinaryFormatReader::new(&data).unwrap();
    assert_eq!(reader.lookup("a"), Some(b"val_a".as_slice()));
    assert_eq!(reader.lookup("b"), Some(b"val_b".as_slice()));
    assert_eq!(reader.lookup("c"), None);
}

#[test]
fn test_plural_cldr_rules() {
    assert_eq!(get_plural_category("en", 1.0), PluralCategory::One);
    assert_eq!(get_plural_category("en", 2.0), PluralCategory::Other);
    assert_eq!(get_plural_category("en", 0.0), PluralCategory::Other);

    assert_eq!(get_plural_category("es", 1.0), PluralCategory::One);
    assert_eq!(get_plural_category("es", 2.0), PluralCategory::Other);

    assert_eq!(get_plural_category("fr", 0.0), PluralCategory::One);
    assert_eq!(get_plural_category("fr", 1.0), PluralCategory::One);
    assert_eq!(get_plural_category("fr", 2.0), PluralCategory::Other);

    // Russian rules
    assert_eq!(get_plural_category("ru", 1.0), PluralCategory::One);
    assert_eq!(get_plural_category("ru", 21.0), PluralCategory::One);
    assert_eq!(get_plural_category("ru", 2.0), PluralCategory::Few);
    assert_eq!(get_plural_category("ru", 4.0), PluralCategory::Few);
    assert_eq!(get_plural_category("ru", 5.0), PluralCategory::Many);
    assert_eq!(get_plural_category("ru", 11.0), PluralCategory::Many);
}

#[test]
fn test_bytecode_formatter_manual() {
    // Bytecode representing:
    // [Text("Hello "), Variable("name"), Text("!")]
    // Text opcode: 0x01, len 6, "Hello "
    // Var opcode: 0x02, len 4, "name"
    // Text opcode: 0x01, len 1, "!"
    let mut bc = Vec::new();
    bc.push(0x01);
    bc.extend_from_slice(&6u32.to_be_bytes());
    bc.extend_from_slice(b"Hello ");

    bc.push(0x02);
    bc.extend_from_slice(&4u32.to_be_bytes());
    bc.extend_from_slice(b"name");

    bc.push(0x01);
    bc.extend_from_slice(&1u32.to_be_bytes());
    bc.extend_from_slice(b"!");

    let mut output = String::new();
    let params = [("name", "John")];
    format_message(&bc, "en", &params, &mut output).unwrap();
    assert_eq!(output, "Hello John!");
}

static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_translate_helper_and_macro() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let mut bc = Vec::new();
    bc.push(0x01);
    bc.extend_from_slice(&6u32.to_be_bytes());
    bc.extend_from_slice(b"Hello ");

    bc.push(0x02);
    bc.extend_from_slice(&4u32.to_be_bytes());
    bc.extend_from_slice(b"name");

    // mock build pak
    let mut data = Vec::new();
    data.extend_from_slice(b"L10N");
    data.extend_from_slice(&1u32.to_be_bytes()); // version
    data.extend_from_slice(&41u32.to_be_bytes()); // index offset (16 + 5 + 20)
    data.extend_from_slice(&1u32.to_be_bytes()); // index count

    // key data
    data.extend_from_slice(b"hello"); // key
    data.extend_from_slice(&bc); // value

    // entry 1: key "hello" (16, 5), val (21, 15)
    data.extend_from_slice(&16u32.to_be_bytes());
    data.extend_from_slice(&5u32.to_be_bytes());
    data.extend_from_slice(&21u32.to_be_bytes());
    data.extend_from_slice(&(bc.len() as u32).to_be_bytes());

    let store = TranslationStore {
        locales: vec![("en".to_string(), Arc::new(data))],
        fallback: "en".to_string(),
    };
    swap_store(store);

    let result = translate(
        "en",
        "hello",
        l10n4x_core::l10n_params! { "name" => "Diego" },
    );
    assert_eq!(result, "Hello Diego");
}

#[test]
#[cfg(feature = "std")]
fn test_lock_free_concurrency_rcu() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let initial_store = TranslationStore {
        locales: vec![("en".to_string(), Arc::new(vec![]))],
        fallback: "en".to_string(),
    };
    swap_store(initial_store);

    let active = Arc::new(core::sync::atomic::AtomicBool::new(true));
    let active_clone = active.clone();

    // Spawn a background locale swapper (dynamic reloads)
    let swapper = thread::spawn(move || {
        while active_clone.load(core::sync::atomic::Ordering::Relaxed) {
            let mut mock_data = Vec::new();
            mock_data.extend_from_slice(b"L10N");
            mock_data.extend_from_slice(&1u32.to_be_bytes());
            mock_data.extend_from_slice(&16u32.to_be_bytes()); // index offset
            mock_data.extend_from_slice(&0u32.to_be_bytes()); // count

            let store = TranslationStore {
                locales: vec![("en".to_string(), Arc::new(mock_data))],
                fallback: "en".to_string(),
            };
            swap_store(store);
            thread::yield_now();
        }
    });

    // Spawn multiple concurrent reader threads
    let mut readers = Vec::new();
    for _ in 0..8 {
        let active_reader = active.clone();
        readers.push(thread::spawn(move || {
            while active_reader.load(core::sync::atomic::Ordering::Relaxed) {
                read_store(|store| {
                    // verify the store is always consistent and doesn't crash
                    let _ = store.lookup("en");
                });
                thread::yield_now();
            }
        }));
    }

    // Run the test for 200 milliseconds
    thread::sleep(std::time::Duration::from_millis(200));

    // Stop thread loops
    active.store(false, core::sync::atomic::Ordering::Relaxed);

    swapper.join().unwrap();
    for reader in readers {
        reader.join().unwrap();
    }
}

#[test]
#[cfg(feature = "std")]
fn test_ebr_stress() {
    let _lock = TEST_MUTEX.lock().unwrap();
    let initial_store = TranslationStore {
        locales: vec![("en".to_string(), Arc::new(vec![]))],
        fallback: "en".to_string(),
    };
    swap_store(initial_store);

    let active = Arc::new(core::sync::atomic::AtomicBool::new(true));
    let active_clone = active.clone();

    // Spawn swapper thread: swaps store every 10ms
    let swapper = thread::spawn(move || {
        let mut count: u32 = 0;
        while active_clone.load(core::sync::atomic::Ordering::Relaxed) {
            let mut mock_data = Vec::new();
            mock_data.extend_from_slice(b"L10N");
            mock_data.extend_from_slice(&1u32.to_be_bytes());
            mock_data.extend_from_slice(&16u32.to_be_bytes());
            mock_data.extend_from_slice(&0u32.to_be_bytes());

            let store = TranslationStore {
                locales: vec![
                    ("en".to_string(), Arc::new(mock_data)),
                    ("es".to_string(), Arc::new(vec![count as u8])),
                ],
                fallback: "en".to_string(),
            };
            swap_store(store);
            count = count.wrapping_add(1);
            thread::sleep(std::time::Duration::from_millis(10));
        }
    });

    // Spawn 4 concurrent reader threads
    let mut readers = Vec::new();
    for i in 0..4 {
        let active_reader = active.clone();
        readers.push(thread::spawn(move || {
            while active_reader.load(core::sync::atomic::Ordering::Relaxed) {
                read_store(|store| {
                    let _ = store.lookup("en");
                    let _ = store.lookup("es");
                    // Read something to stress memory access
                    if let Some(buf) = store.lookup("es") {
                        if !buf.is_empty() {
                            let _val = buf[0];
                        }
                    }
                });
                if i % 2 == 0 {
                    thread::yield_now();
                }
            }
        }));
    }

    // Run for 5 seconds
    thread::sleep(std::time::Duration::from_secs(5));

    active.store(false, core::sync::atomic::Ordering::Relaxed);

    swapper.join().unwrap();
    for reader in readers {
        reader.join().unwrap();
    }
}
