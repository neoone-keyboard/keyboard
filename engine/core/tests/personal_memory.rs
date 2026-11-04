//! The learned-word store's memory with every table at its cap: the keyboard extension has a
//! 32 MB gate, and this must stay a small part of it. One test in this file, so that the
//! counting allocator sees nothing else.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use mlengine_core::personal::{
    CHOICE_CAPACITY, Habit, PAIR_CAPACITY, TRIGRAM_CAPACITY, USAGE_CAPACITY,
};
use mlengine_core::user::{CAPACITY, UserStore};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(
            new_size as isize - layout.size() as isize,
            Ordering::Relaxed,
        );
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// A word of a typical length (7 letters), distinct for each `i`.
fn word(prefix: char, i: usize) -> String {
    format!("{prefix}{i:06}")
}

#[test]
fn a_full_store_takes_little_memory() {
    let before = LIVE.load(Ordering::Relaxed);
    let mut store = UserStore::in_memory();
    for i in 0..CAPACITY {
        store.learn(&word('l', i));
    }
    let words = LIVE.load(Ordering::Relaxed) - before;
    // Pairs and triples past their caps, each context with two followers, as typing makes.
    for i in 0..PAIR_CAPACITY.max(TRIGRAM_CAPACITY) {
        let (a, b) = (word('a', i / 2), word('b', i / 2));
        store.note_used(
            &word('c', i),
            Some(&b),
            Some(&a),
            true,
            Some(Habit::English),
        );
    }
    for i in 0..USAGE_CAPACITY {
        store.note_used(&word('u', i), None, None, true, None);
    }
    for i in 0..CHOICE_CAPACITY {
        store.note_choice(&word('t', i), &word('k', i));
    }
    let total = LIVE.load(Ordering::Relaxed) - before;
    println!(
        "learned words {words} bytes; with phrases, uses and choices at their caps {total} bytes"
    );
    assert!(total < 2_000_000, "{total} bytes");
    drop(store);
}
