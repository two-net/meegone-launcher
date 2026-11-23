//! The Rust app's share of each performance budget (SRS 6.1, ADR 0041), measured on the host
//! with fake platform adapters and 300 apps, so CI notices when a change makes the core logic
//! slower. `cargo bench --bench budgets` prints a Markdown table and fails if a median goes past
//! its limit.
//!
//! The fakes cost nothing, so only MeeGone's own work is timed: never JNI, Android's views or
//! the display. Each limit is a tenth of the device budget, which leaves the host's speed and a
//! busy CI runner plenty of room. The budgets themselves are measured on a device, by
//! `tools/perf-bench.sh`. PERF-07, memory, has no share here: Java's views and icons hold most
//! of it, and only the device can tell.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use meegone_core_data::fakes::{FakeStorage, Fakes, app};
use meegone_core_data::{App, Rect};
use meegone_launcher::{
    Bounds, GridLayout, Launcher, Layout, PointerEvent, PointerKind, ShellIntent, Surface,
};

/// As many apps as the budgets name (PERF-04, PERF-05, PERF-07).
const APPS: usize = 300;

/// A phone 1080 × 2400 px at 2.75 px per dp, as in the acceptance tests.
const WIDTH: f64 = 1080.0;
const HEIGHT: f64 = 2400.0;

const PHONE: Layout = Layout {
    width_px: WIDTH,
    density: 2.75,
    touch_slop_px: 22.0,
    rtl: false,
};

/// The phone's grid as `AppsGrid` reports it: four columns of 248 px, rows 308 px apart.
const PHONE_GRID: GridLayout = GridLayout {
    width_px: 992.0,
    row_height_px: 308.0,
    density: 2.75,
    top_px: 82.0,
    bottom_px: 132.0,
    height_px: HEIGHT,
    left_px: 44.0,
    view_top_px: 0.0,
    icon_px: 154.0,
    icon_top_px: 22.0,
};

/// The search field's band on that phone, below the status bar.
const SEARCH_BAND: Bounds = Bounds {
    left: 0.0,
    top: 82.0,
    right: WIDTH,
    bottom: 258.0,
};

/// An icon on screen, which a launch grows from (ANIM-08).
const ICON: Rect = Rect {
    left: 44,
    top: 300,
    right: 198,
    bottom: 454,
};

/// One frame at 120 Hz, in ms.
const FRAME_MS: u64 = 8;

/// One budget's share, timed: its median and worst run against its limit.
struct Row {
    budget: &'static str,
    scenario: &'static str,
    median: Duration,
    worst: Duration,
    limit: Duration,
}

fn main() -> ExitCode {
    let rows = [
        home_from_an_app(),
        cold_start_with_a_saved_layout(),
        swipe_frame(),
        keystroke(),
        tap_to_launch(),
    ];
    let stored = stored_bytes();
    let stored_limit = 5 * 1024 * 1024;

    println!("| Budget | Scenario, {APPS} apps | Median | Worst | Limit | |");
    println!("|---|---|---|---|---|---|");
    let mut failed = 0;
    for row in &rows {
        let pass = row.median <= row.limit;
        failed += usize::from(!pass);
        println!(
            "| {} | {} | {} | {} | {} | {} |",
            row.budget,
            row.scenario,
            shown(row.median),
            shown(row.worst),
            shown(row.limit),
            verdict(pass),
        );
    }
    let pass = stored <= stored_limit;
    failed += usize::from(!pass);
    println!(
        "| PERF-08 | Stored data: the layout and settings | {} KiB | | {} KiB | {} |",
        stored.div_ceil(1024),
        stored_limit / 1024,
        verdict(pass),
    );
    if failed > 0 {
        eprintln!("{failed} of the Rust app's budget shares went past their limits (ADR 0041).");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// PERF-01: an app was open and Home brings the home back. The host sends `Resumed` and
/// `Returned`, then its layout, and reads the frame each time (`nativeResumed`, `onResumed`).
fn home_from_an_app() -> Row {
    let fakes = with_apps();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    let samples = time(300, || {
        let started = Instant::now();
        launcher.dispatch(ShellIntent::Resumed);
        launcher.dispatch(ShellIntent::Returned { home: true });
        launcher.frame();
        launcher.dispatch(ShellIntent::Layout(PHONE));
        read_cells(&mut launcher, Surface::Grid);
        let took = started.elapsed();
        come_to_rest(&mut launcher, &mut clock);
        took
    });
    row(
        "PERF-01",
        "Home from an app: Resumed, Returned and the first frame",
        &samples,
        30,
    )
}

/// PERF-02 and PERF-04: a cold start with a saved layout. A new launcher on the stored files
/// lays out, builds its first frame and gives every cell to the grid (`nativeStart`,
/// `nativeLayout`, `nativeRead`, `nativeAppsCells`).
fn cold_start_with_a_saved_layout() -> Row {
    let fakes = with_apps();
    // The first start saves the layout every later start reads.
    drop(on_a_phone(&fakes));
    let saved = fakes.storage.files.lock().unwrap().clone();
    let samples = time(50, || {
        let storage = FakeStorage::default();
        *storage.files.lock().unwrap() = saved.clone();
        let fakes = Fakes {
            storage: Arc::new(storage),
            ..fakes.clone()
        };
        let started = Instant::now();
        let mut launcher = Launcher::start(fakes.platform());
        launcher.dispatch(ShellIntent::Layout(PHONE));
        launcher.view().apps().grid_laid_out(PHONE_GRID);
        let cells = read_cells(&mut launcher, Surface::Grid);
        let took = started.elapsed();
        assert_eq!(cells, APPS, "the saved layout shows every app");
        took
    });
    row(
        "PERF-04",
        "Cold start: the saved layout's first frame and cells",
        &samples,
        50,
    )
}

/// PERF-03: one frame while a finger drags the views sideways. The touch is applied and read
/// in one call, then the vsync advances the motion and the frame is read again.
fn swipe_frame() -> Row {
    let fakes = with_apps();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    let mut samples = Vec::new();
    let mut views = Vec::new();
    for _ in 0..20 {
        views.push(launcher.shell().state().current);
        touch(&mut launcher, PointerKind::Down, 900.0, clock);
        for step in 1..=40u32 {
            clock += FRAME_MS;
            let x = 900.0 - 700.0 * f64::from(step) / 40.0;
            let started = Instant::now();
            touch(&mut launcher, PointerKind::Move, x, clock);
            launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
            black_box(launcher.frame());
            samples.push(started.elapsed());
        }
        clock += FRAME_MS;
        touch(&mut launcher, PointerKind::Up, 200.0, clock);
        // The spring that settles the views is a frame each too.
        while launcher.frame().animating {
            clock += FRAME_MS;
            let started = Instant::now();
            launcher.dispatch(ShellIntent::Frame(Duration::from_millis(clock)));
            black_box(launcher.frame());
            samples.push(started.elapsed());
        }
    }
    views.dedup();
    assert!(views.len() > 10, "each swipe moves to the next view");
    samples.sort();
    Row {
        budget: "PERF-03",
        scenario: "One frame of a view swipe and its spring",
        median: samples[samples.len() / 2],
        worst: samples[samples.len() - 1],
        limit: Duration::from_micros(830),
    }
}

/// PERF-05: one keystroke in search. The field's new text goes in, the frame is built and the
/// results are given to their grid (`nativeSearchChanged`, `nativeAppsCells`).
fn keystroke() -> Row {
    let fakes = with_apps();
    let (mut launcher, mut clock) = on_a_phone(&fakes);
    launcher.view().apps().search_laid_out(
        GridLayout {
            top_px: SEARCH_BAND.bottom,
            ..PHONE_GRID
        },
        SEARCH_BAND,
    );
    launcher.frame();
    open_search(&mut launcher, &mut clock);
    let queries = [
        "m", "ma", "map", "maps", "c", "ca", "cam", "cl", "clo", "s", "se", "x",
    ];
    let mut next = queries.iter().cycle();
    let samples = time(600, || {
        let query = next.next().unwrap();
        let started = Instant::now();
        launcher.view().apps().search_changed((*query).to_owned());
        read_cells(&mut launcher, Surface::Search);
        started.elapsed()
    });
    assert!(
        launcher.view().apps().query().is_some(),
        "search stays open"
    );
    row(
        "PERF-05",
        "One keystroke in search, to its results",
        &samples,
        5,
    )
}

/// PERF-06: a tap on an app's cell, until the launch request has gone to the platform
/// (`nativeAppTapped`).
fn tap_to_launch() -> Row {
    let fakes = with_apps();
    let (mut launcher, _) = on_a_phone(&fakes);
    let mut index = 0;
    let samples = time(300, || {
        let revision = launcher.frame().apps.revision;
        index = (index + 7) % APPS;
        let started = Instant::now();
        let tapped = launcher
            .view()
            .apps()
            .cell_tapped(Surface::Grid, revision, index, ICON);
        launcher.frame();
        let took = started.elapsed();
        assert!(tapped, "the tap reaches the cell");
        took
    });
    // Every run launched, the warm-up's too.
    assert_eq!(fakes.apps.launched.lock().unwrap().len(), 330);
    row(
        "PERF-06",
        "A tap on an app, until the launch request leaves",
        &samples,
        5,
    )
}

/// PERF-08: every byte MeeGone stores with 300 apps, once it has settled. The image cache's two
/// images are the crops, which `FileStorage`'s 50 MB budget also covers (ADR 0004, ADR 0028).
fn stored_bytes() -> u64 {
    let fakes = with_apps();
    drop(on_a_phone(&fakes));
    let files = fakes.storage.files.lock().unwrap();
    files.values().map(|bytes| bytes.len() as u64).sum()
}

/// A device after setup with 300 apps, their labels made of a handful of common words.
fn with_apps() -> Fakes {
    const WORDS: [&str; 12] = [
        "Mail", "Maps", "Camera", "Clock", "Photos", "Music", "Notes", "Weather", "Bank", "Chess",
        "Radio", "Store",
    ];
    let fakes = Fakes::after_setup();
    let apps: Vec<App> = (0..APPS)
        .map(|i| {
            let label = format!(
                "{} {}",
                WORDS[i % WORDS.len()],
                WORDS[(i / 7) % WORDS.len()]
            );
            app(&format!("com.example.app{i:03}"), &format!("{label} {i}"))
        })
        .collect();
    *fakes.apps.apps.lock().unwrap() = apps;
    fakes
}

/// A launcher laid out on the phone with its grid, at rest; and the time in ms.
fn on_a_phone(fakes: &Fakes) -> (Launcher, u64) {
    let mut launcher = Launcher::start(fakes.platform());
    launcher.dispatch(ShellIntent::Layout(PHONE));
    launcher.view().apps().grid_laid_out(PHONE_GRID);
    read_cells(&mut launcher, Surface::Grid);
    let mut clock = 0;
    come_to_rest(&mut launcher, &mut clock);
    (launcher, clock)
}

/// Builds the frame and reads a surface's cells as the host does when their revision changes:
/// each one's fields as text. Returns how many there were.
fn read_cells(launcher: &mut Launcher, surface: Surface) -> usize {
    black_box(launcher.frame());
    let fields: Vec<(String, String)> = launcher
        .view()
        .apps()
        .cells(surface)
        .map(|cell| {
            let id = cell
                .app
                .map(|id| format!("{}/{}", id.package, id.activity))
                .unwrap_or_default();
            (cell.label.to_owned(), id)
        })
        .collect();
    black_box(fields).len()
}

fn touch(launcher: &mut Launcher, kind: PointerKind, x: f64, at: u64) {
    launcher.dispatch(ShellIntent::Pointer(PointerEvent {
        kind,
        x,
        y: 1200.0,
        at: Duration::from_millis(at),
    }));
    black_box(launcher.frame());
}

/// Pulls the search field open from the top of Applications, past its whole band (SRCH-01).
fn open_search(launcher: &mut Launcher, clock: &mut u64) {
    let (from, to) = (600.0, 600.0 + 22.0 + 176.0 + 60.0);
    let pull = |launcher: &mut Launcher, kind, y, at| {
        launcher.dispatch(ShellIntent::Pointer(PointerEvent {
            kind,
            x: 540.0,
            y,
            at: Duration::from_millis(at),
        }));
        launcher.frame();
    };
    pull(launcher, PointerKind::Down, from, *clock);
    for step in 1..=40u32 {
        let y = from + (to - from) * f64::from(step) / 40.0;
        pull(launcher, PointerKind::Move, y, *clock + u64::from(step) * 8);
    }
    *clock += 420;
    pull(launcher, PointerKind::Up, to, *clock);
    come_to_rest(launcher, clock);
}

/// The display draws frames until nothing moves.
fn come_to_rest(launcher: &mut Launcher, clock: &mut u64) {
    while launcher.frame().animating {
        *clock += FRAME_MS;
        launcher.dispatch(ShellIntent::Frame(Duration::from_millis(*clock)));
    }
}

/// Runs `sample` `runs` times, after a tenth as many to warm up, and returns the times sorted.
fn time(runs: usize, mut sample: impl FnMut() -> Duration) -> Vec<Duration> {
    for _ in 0..runs / 10 {
        sample();
    }
    let mut samples: Vec<Duration> = (0..runs).map(|_| sample()).collect();
    samples.sort();
    samples
}

fn row(budget: &'static str, scenario: &'static str, sorted: &[Duration], limit_ms: u64) -> Row {
    Row {
        budget,
        scenario,
        median: sorted[sorted.len() / 2],
        worst: sorted[sorted.len() - 1],
        limit: Duration::from_millis(limit_ms),
    }
}

fn shown(duration: Duration) -> String {
    if duration < Duration::from_millis(1) {
        format!("{:.1} µs", duration.as_nanos() as f64 / 1000.0)
    } else {
        format!("{:.1} ms", duration.as_secs_f64() * 1000.0)
    }
}

fn verdict(pass: bool) -> &'static str {
    if pass { "✅" } else { "❌ over" }
}
