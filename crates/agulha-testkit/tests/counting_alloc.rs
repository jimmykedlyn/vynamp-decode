use agulha_testkit::alloc::{CountingAlloc, allocations_on_this_thread};

#[global_allocator]
static A: CountingAlloc = CountingAlloc;

#[test]
fn counts_allocations_on_this_thread() {
    let before = allocations_on_this_thread();
    let v: Vec<u8> = Vec::with_capacity(1024);
    std::hint::black_box(&v);
    let after = allocations_on_this_thread();
    assert!(after > before);

    let before = allocations_on_this_thread();
    let x = std::hint::black_box(1 + 1);
    assert_eq!(x, 2);
    assert_eq!(allocations_on_this_thread(), before);
}
