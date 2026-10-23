//! Heap size of a CompiledRuleset: live bytes of the process after compiling
//! minus before (a counting global allocator; the source rules are consumed
//! and freed by the compiler, so the delta is what the compiled ruleset
//! keeps). Prints the pre-classic (legacy) and classic rulesets.
//!
//! cargo run --release --offline -p classic-content --example rules_size

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

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

fn measure(name: &str, source: impl Fn() -> classic_rules::rules::SourceRules) {
    let parsed_before = LIVE.load(Ordering::Relaxed);
    let parsed = source();
    let source_bytes = LIVE.load(Ordering::Relaxed) - parsed_before;
    drop(parsed);
    let before = LIVE.load(Ordering::Relaxed);
    let rules = classic_rules::compile_rules(source()).expect("ruleset compiles");
    let compiled_bytes = LIVE.load(Ordering::Relaxed) - before;
    let inert: usize = rules.units.iter().map(|u| u.inert.len()).sum::<usize>()
        + rules.buildings.iter().map(|b| b.inert.len()).sum::<usize>();
    println!(
        "m rules.size.{name} compiled_heap_bytes={compiled_bytes} source_heap_bytes={source_bytes} \
         units={} buildings={} technologies={} promotions={} unit_building_inert={inert} stack_bytes={}",
        rules.units.len(),
        rules.buildings.len(),
        rules.technologies.len(),
        rules.promotions.len(),
        std::mem::size_of_val(&rules),
    );
}

fn main() {
    measure("legacy", classic_content::legacy::legacy_rules);
    measure("classic", classic_content::core::core_rules);
}
