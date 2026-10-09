//! F11-FR7: what a layout can make the renderer allocate, measured. A counting allocator (this
//! test binary only) tracks the live bytes; every allowed filter, and every pair of them chained
//! 40 times, on a few seeds, must peak under 32 MB. Allocations past 256 MB are refused, so a
//! regression aborts this binary instead of exhausting the machine.
use meeting::templates::{render, sample_context};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
const REFUSE_PAST: usize = 256 * 1024 * 1024;
const LIMIT: usize = 32 * 1024 * 1024;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(l.size(), Ordering::SeqCst) + l.size();
        if now > REFUSE_PAST {
            LIVE.fetch_sub(l.size(), Ordering::SeqCst);
            return std::ptr::null_mut();
        }
        PEAK.fetch_max(now, Ordering::SeqCst);
        // SAFETY: forwarded unchanged to the system allocator.
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::SeqCst);
        // SAFETY: `p` came from `alloc` with this layout.
        unsafe { System.dealloc(p, l) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// Bytes allocated above what was live before `layout` rendered, at its peak.
fn peak_of(layout: &str) -> usize {
    let ctx = sample_context();
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let _ = render(layout, &ctx);
    PEAK.load(Ordering::SeqCst) - base
}

const FILTERS: [&str; 10] = [
    "default('x')",
    "upper",
    "lower",
    "title",
    "capitalize",
    "trim",
    "length",
    "first",
    "last",
    "join(', ')",
];

const SEEDS: [&str; 4] = ["'ab'", r#"'\\"&'"#, "transcript", "todos"];

/// The constructions reviewers used to build huge values, at depth 40.
fn hostile() -> Vec<String> {
    let rep = |s: &str, n: usize| s.repeat(n);
    let nest = |open: &str, close: &str, n: usize| {
        let mut s = "summary".to_owned();
        for _ in 0..n {
            s = format!("{open}{s}{close}");
        }
        format!("{{{{ {s}|length }}}}")
    };
    vec![
        format!("{{{{ summary{}|length }}}}", rep("|pprint", 40)),
        format!("{{{{ '\\\\'{}|length }}}}", rep("|pprint", 40)),
        nest("[", "]|string", 40),
        format!("{{{{ 'ab'{}|length }}}}", rep("|sort|e", 40)),
        format!(
            "{{{{ summary{}|length }}}}",
            rep("|map('pprint')|string", 40)
        ),
        nest("(", ")|upper", 30),
        nest("(", ")|join", 30),
    ]
}

#[test]
fn every_allowed_filter_and_pair_stays_small() {
    let mut worst = (0, String::new());
    let mut note = |layout: String| {
        let p = peak_of(&layout);
        if p > worst.0 {
            worst = (p, layout);
        }
    };
    for seed in SEEDS {
        for f in FILTERS {
            note(format!("{{{{ {seed}|{f} }}}}"));
            for g in FILTERS {
                let chain = format!("|{f}|{g}").repeat(20);
                note(format!("{{{{ {seed}{chain} }}}}"));
            }
        }
        // Nesting depth 30: parentheses, and if/for blocks.
        let parens = format!("{}{seed}{}", "(".repeat(30), ")|first".repeat(30));
        note(format!("{{{{ {parens} }}}}"));
        let blocks = format!(
            "{}{{{{ {seed}|length }}}}{}",
            "{% for x in transcript %}{% if x %}".repeat(15),
            "{% endif %}{% endfor %}".repeat(15)
        );
        note(blocks);
    }
    for l in hostile() {
        note(l);
    }
    assert!(worst.0 < LIMIT, "{} bytes for {}", worst.0, worst.1);
}
