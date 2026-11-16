use meegone_apps_state::{Bounds, FolderId, GridItem, GridLayout, Surface};
use meegone_core_data::fakes::{app, app_id};
use meegone_core_state::motion::MAX_OVERSHOOT;

use super::*;

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn at(kind: PointerKind, x: f64, y: f64, millis: u64) -> PointerEvent {
    PointerEvent {
        kind,
        x,
        y,
        at: ms(millis),
    }
}

const SLOP: f64 = 16.0;

fn key(index: usize) -> CellKey {
    CellKey::App(app_id(&format!("app{index:02}")))
}

fn apps(count: usize) -> Arc<[GridItem]> {
    (0..count)
        .map(|index| GridItem::App {
            app: app(&format!("app{index:02}"), "App"),
            new: false,
        })
        .collect()
}

/// 4 columns of 250 px from 50 px in, rows of 300 px, 100 px of safe area above and below,
/// 1000 px tall: 800 px of grid show. Each icon is 140 px from 20 px down its cell, so its
/// centre is 125 px in and 90 px down. 2.5 px per dp: the auto-scroll zones are 160 px deep,
/// and it scrolls up to 1600 px a second. A dragged app's centre within 56 px of an icon's is
/// over it.
fn layout() -> GridLayout {
    GridLayout {
        width_px: 1000.0,
        row_height_px: 300.0,
        density: 2.5,
        top_px: 100.0,
        bottom_px: 100.0,
        height_px: 1000.0,
        left_px: 50.0,
        view_top_px: 0.0,
        icon_px: 140.0,
        icon_top_px: 20.0,
    }
}

/// 40 apps in 11 rows, scrolled `scroll_px`; no folder open.
fn grid(scroll_px: f64) -> DragSurfaces {
    DragSurfaces {
        grid: SurfaceView {
            layout: Some(layout()),
            scroll_px,
            cells: apps(40),
            rows: 11,
        },
        folder: None,
        folder_bounds: None,
        rtl: false,
        scale: 1.0,
        rest_scale: 1.0,
    }
}

/// Not laid out yet: the icon only follows the finger.
fn unlaid() -> DragSurfaces {
    DragSurfaces {
        grid: SurfaceView {
            layout: None,
            ..grid(0.0).grid
        },
        ..grid(0.0)
    }
}

/// Where cell `place`'s icon centre is on screen, unscrolled.
fn icon(place: usize) -> (f64, f64) {
    let (column, row) = ((place % 4) as f64, (place / 4) as f64);
    (50.0 + column * 250.0 + 125.0, 100.0 + row * 300.0 + 90.0)
}

/// App 5 (row 1, column 1), picked up by a finger on its icon's centre that moved to
/// (`x`, `y`) at time 0.
fn picked_up(surfaces: &DragSurfaces, x: f64, y: f64) -> Lift {
    let mut lift = Lift::default();
    lift.arm(key(5), Surface::Grid, icon(5));
    assert_eq!(
        lift.pointer(&at(PointerKind::Move, x, y, 0), SLOP, surfaces, 1.0),
        Some(LiftEvent::PickedUp)
    );
    lift
}

fn finger(lift: &mut Lift, surfaces: &DragSurfaces, x: f64, y: f64, millis: u64) {
    lift.pointer(&at(PointerKind::Move, x, y, millis), SLOP, surfaces, 1.0);
}

#[test]
fn moving_past_the_slop_picks_the_icon_up_and_it_follows_the_finger() {
    // APPS-11, ANIM-13: not laid out, so its centre is the finger.
    let surfaces = unlaid();
    let mut lift = Lift::default();
    lift.arm(key(5), Surface::Grid, (100.0, 200.0));
    assert_eq!(lift.armed(), Some((&key(5), Surface::Grid)));
    assert_eq!(lift.state(), LiftState::default());

    // Within the slop, either way, it stays a long-press.
    let within = at(PointerKind::Move, 116.0, 184.0, 0);
    assert_eq!(lift.pointer(&within, SLOP, &surfaces, 1.0), None);
    assert!(!lift.is_lifted());

    let past = at(PointerKind::Move, 100.0, 217.0, 0);
    assert_eq!(
        lift.pointer(&past, SLOP, &surfaces, 1.0),
        Some(LiftEvent::PickedUp)
    );
    assert!(lift.is_lifted());
    assert_eq!(lift.armed(), None);
    assert_eq!(lift.held(), Some((&key(5), Surface::Grid)));
    let state = lift.state();
    assert_eq!((state.key, state.x, state.y), (Some(key(5)), 100.0, 217.0));
    // It lifts from its cell's size to 110 %.
    assert_eq!(state.scale, 1.0);
    assert!(state.animating);

    finger(&mut lift, &surfaces, 40.0, 500.0, 0);
    let state = lift.state();
    assert_eq!((state.x, state.y), (40.0, 500.0));
}

#[test]
fn the_icon_stays_where_the_finger_holds_it() {
    // Held by a corner, its centre keeps that distance from the finger.
    let surfaces = grid(0.0);
    let mut lift = Lift::default();
    let (x, y) = icon(5);
    lift.arm(key(5), Surface::Grid, (x - 30.0, y + 20.0));
    lift.pointer(
        &at(PointerKind::Move, 600.0, 900.0, 0),
        SLOP,
        &surfaces,
        1.0,
    );
    let state = lift.state();
    assert_eq!((state.x, state.y), (630.0, 880.0));
}

#[test]
fn the_icon_lifts_to_110_percent_on_a_spring() {
    // ANIM-13: from edit mode's 92 %.
    let surfaces = DragSurfaces {
        scale: 0.92,
        ..unlaid()
    };
    let mut lift = Lift::default();
    lift.arm(key(5), Surface::Grid, (0.0, 0.0));
    lift.pointer(&at(PointerKind::Move, 0.0, 50.0, 0), SLOP, &surfaces, 1.0);
    assert_eq!(lift.state().scale, 0.92);

    lift.frame(ms(0), &surfaces, 1.0);
    lift.frame(ms(60), &surfaces, 1.0);
    let midway = lift.state().scale;
    assert!(midway > 0.92 && midway < DRAG_LIFT_SCALE, "{midway}");
    for t in (60..=400).step_by(8) {
        lift.frame(ms(t), &surfaces, 1.0);
    }
    let state = lift.state();
    assert_eq!(state.scale, DRAG_LIFT_SCALE);
    // Lifted and still, with nothing to wait for: no more frames.
    assert!(!state.animating);
}

#[test]
fn a_cancel_or_a_new_finger_puts_the_icon_back() {
    for end in [PointerKind::Cancel, PointerKind::Down] {
        let surfaces = unlaid();
        let mut lift = picked_up(&surfaces, 500.0, 400.0);

        assert_eq!(
            lift.pointer(&at(end, 300.0, 200.0, 0), SLOP, &surfaces, 1.0),
            None
        );

        assert_eq!(lift.state(), LiftState::default());
        assert!(!lift.is_lifted());
        // A later move picks nothing up.
        let later = at(PointerKind::Move, 0.0, 0.0, 0);
        assert_eq!(lift.pointer(&later, SLOP, &surfaces, 1.0), None);
    }
}

#[test]
fn a_long_press_that_ends_without_a_drag_picks_nothing_up() {
    let surfaces = unlaid();
    let mut lift = Lift::default();
    lift.arm(key(5), Surface::Grid, (100.0, 200.0));

    lift.pointer(&at(PointerKind::Up, 100.0, 200.0, 0), SLOP, &surfaces, 1.0);

    assert_eq!(lift.armed(), None);
    let later = at(PointerKind::Move, 300.0, 200.0, 0);
    assert_eq!(lift.pointer(&later, SLOP, &surfaces, 1.0), None);
}

#[test]
fn a_place_the_icon_stays_over_becomes_its_own() {
    // APPS-12: from place 5 to place 1's label, a row up, after REORDER_DWELL.
    let surfaces = grid(0.0);
    let (x, y) = icon(1);
    let mut lift = picked_up(&surfaces, x, y + 170.0);
    let step = lift.frame(ms(0), &surfaces, 1.0);
    assert_eq!(step.arrange, None);
    assert!(lift.state().animating);

    let step = lift.frame(REORDER_DWELL - ms(1), &surfaces, 1.0);
    assert_eq!(step.arrange, None);
    let step = lift.frame(REORDER_DWELL, &surfaces, 1.0);
    assert_eq!(step.arrange, Some(DropAt::Grid { key: key(5), to: 1 }));
}

#[test]
fn crossing_cells_quickly_takes_no_place_on_the_way() {
    // APPS-12: each new place starts its wait again.
    let surfaces = grid(0.0);
    let (_, y) = icon(5);
    let mut lift = picked_up(&surfaces, 700.0, y + 170.0);
    let mut taken = Vec::new();
    for (i, x) in [700.0, 950.0, 700.0, 425.0].into_iter().enumerate() {
        let t = 100 * i as u64;
        finger(&mut lift, &surfaces, x, y + 170.0, t);
        taken.extend(lift.frame(ms(t + 99), &surfaces, 1.0).arrange);
    }
    assert!(taken.is_empty(), "{taken:?}");
}

#[test]
fn a_finger_resting_on_a_line_between_cells_keeps_its_place() {
    // The line between places 6 and 7 is at x = 800. A little either side of it, below the
    // icons, stays over 6, which keeps its wait.
    let surfaces = grid(0.0);
    let (_, y) = icon(6);
    let below = y + 170.0;
    let mut lift = picked_up(&surfaces, 795.0, below);
    lift.frame(ms(0), &surfaces, 1.0);
    for (i, x) in [805.0, 797.0, 810.0, 802.0].into_iter().enumerate() {
        finger(&mut lift, &surfaces, x, below, 40 * (i as u64 + 1));
    }
    assert_eq!(
        lift.frame(ms(200), &surfaces, 1.0).arrange,
        Some(DropAt::Grid { key: key(5), to: 6 })
    );
}

#[test]
fn a_gap_beside_an_icon_takes_its_place() {
    // ADR 0015: the merge zone is the icon's middle; its sides still reorder.
    let surfaces = grid(0.0);
    let (x, y) = icon(6);
    let mut lift = picked_up(&surfaces, x - 110.0, y);
    lift.frame(ms(0), &surfaces, 1.0);
    assert_eq!(lift.hover(), HoverState::default());
    assert_eq!(
        lift.frame(REORDER_DWELL, &surfaces, 1.0).arrange,
        Some(DropAt::Grid { key: key(5), to: 6 })
    );
}

#[test]
fn holding_an_app_over_another_arms_a_folder_and_the_icon_grows_then_pulses() {
    // FOLD-01, ANIM-14
    let surfaces = grid(0.0);
    let (x, y) = icon(6);
    let mut lift = picked_up(&surfaces, x + 20.0, y);
    lift.frame(ms(0), &surfaces, 1.0);
    let hover = lift.hover();
    assert_eq!(
        (hover.key, hover.armed, hover.scale),
        (Some(key(6)), false, 1.0)
    );

    // It never takes the place under it.
    lift.frame(ms(200), &surfaces, 1.0);
    assert_eq!(lift.frame(ms(399), &surfaces, 1.0).arrange, None);
    let growing = lift.hover().scale;
    assert!(growing > 1.0 && growing < FOLDER_HOVER_SCALE, "{growing}");

    lift.frame(FOLDER_HOVER, &surfaces, 1.0);
    let armed = lift.hover();
    assert!(armed.armed);
    assert!((armed.scale - FOLDER_HOVER_SCALE).abs() < 1e-9);
    lift.frame(FOLDER_HOVER + FOLDER_PULSE / 2, &surfaces, 1.0);
    assert!((lift.hover().scale - FOLDER_PULSE_SCALE).abs() < 1e-9);
    assert!(lift.state().animating);

    let dropped = lift.pointer(&at(PointerKind::Up, x + 20.0, y, 700), SLOP, &surfaces, 1.0);
    assert_eq!(
        dropped,
        Some(LiftEvent::Dropped(DropAt::Combine {
            app: app_id("app05"),
            onto: key(6)
        }))
    );
}

#[test]
fn a_drop_on_an_icon_before_it_is_armed_puts_the_app_back() {
    // ADR 0015: neither a folder nor that icon's place.
    let surfaces = grid(0.0);
    let (x, y) = icon(6);
    let mut lift = picked_up(&surfaces, x, y);
    lift.frame(ms(0), &surfaces, 1.0);

    let dropped = lift.pointer(&at(PointerKind::Up, x, y, 300), SLOP, &surfaces, 1.0);

    assert_eq!(dropped, Some(LiftEvent::Dropped(DropAt::Back)));
}

#[test]
fn moving_from_an_icon_to_its_label_starts_the_wait_again() {
    // ADR 0015: armed time over the icon doesn't count towards taking the place.
    let surfaces = grid(0.0);
    let (x, y) = icon(6);
    let mut lift = picked_up(&surfaces, x, y);
    lift.frame(ms(0), &surfaces, 1.0);
    lift.frame(ms(300), &surfaces, 1.0);
    finger(&mut lift, &surfaces, x, y + 170.0, 300);
    assert_eq!(lift.frame(ms(400), &surfaces, 1.0).arrange, None);
    assert_eq!(lift.hover(), HoverState::default());
    assert!(lift.frame(ms(450), &surfaces, 1.0).arrange.is_some());
}

#[test]
fn a_dragged_folder_never_goes_into_anything() {
    // FOLD-05: over an icon it only takes the place.
    let mut surfaces = grid(0.0);
    let mut cells: Vec<GridItem> = apps(40).to_vec();
    cells[5] = GridItem::Folder {
        id: FolderId(1),
        name: String::new(),
        apps: vec![app("a", "A"), app("b", "B")],
    };
    surfaces.grid.cells = cells.into();
    let mut lift = Lift::default();
    lift.arm(CellKey::Folder(FolderId(1)), Surface::Grid, icon(5));
    let (x, y) = icon(6);
    lift.pointer(&at(PointerKind::Move, x, y, 0), SLOP, &surfaces, 1.0);
    lift.frame(ms(0), &surfaces, 1.0);

    assert_eq!(lift.hover(), HoverState::default());
    assert_eq!(
        lift.frame(REORDER_DWELL, &surfaces, 1.0).arrange,
        Some(DropAt::Grid {
            key: CellKey::Folder(FolderId(1)),
            to: 6
        })
    );
}

#[test]
fn near_an_edge_the_grid_scrolls_faster_the_deeper_and_stops_at_its_end() {
    // APPS-12: the bottom zone is the last 160 px of the 800 px that show. The icon's centre
    // starts 390 px down the visible grid; 260 px further is 10 px into the zone.
    let surfaces = grid(0.0);
    let (x, y) = icon(5);
    let mut lift = picked_up(&surfaces, x, y + 260.0);
    lift.frame(ms(0), &surfaces, 1.0);
    let slow = lift.frame(ms(16), &surfaces, 1.0).scroll_to.unwrap();
    assert_eq!(slow.0, Surface::Grid);
    assert!((slow.1 - 100.0 * 0.016).abs() < 0.01, "{slow:?}");

    finger(&mut lift, &surfaces, x, y + 600.0, 16);
    let fast = lift.frame(ms(32), &surfaces, 1.0).scroll_to.unwrap().1;
    // At the edge itself, and past it: 640 dp a second.
    assert!((fast - 1600.0 * 0.016).abs() < 0.01, "{fast}");
    assert!(lift.state().animating);

    // A late frame scrolls no further than two would.
    let late = lift.frame(ms(500), &surfaces, 1.0).scroll_to.unwrap().1;
    assert!((late - 1600.0 * 0.034).abs() < 0.01, "{late}");

    // At the end it scrolls no further, and the icon stays under the finger.
    let end = grid(2500.0);
    lift.sync(&end);
    assert_eq!(lift.frame(ms(516), &end, 1.0).scroll_to, None);
    let state = lift.state();
    assert_eq!((state.x, state.y), (x, y + 600.0));
}

#[test]
fn the_grid_scrolls_up_near_the_top_but_not_past_it() {
    // Place 5's icon shows 190 px down the visible grid; the finger moves up 1000 px.
    let surfaces = grid(200.0);
    let mut lift = Lift::default();
    let (x, y) = icon(5);
    lift.arm(key(5), Surface::Grid, (x, y - 200.0));
    lift.pointer(
        &at(PointerKind::Move, x, y - 1200.0, 0),
        SLOP,
        &surfaces,
        1.0,
    );
    lift.frame(ms(0), &surfaces, 1.0);
    let up = lift.frame(ms(16), &surfaces, 1.0).scroll_to.unwrap().1;
    assert!((up - (200.0 - 1600.0 * 0.016)).abs() < 0.01, "{up}");
    let top = grid(0.0);
    lift.sync(&top);
    assert_eq!(lift.frame(ms(32), &top, 1.0).scroll_to, None);
}

#[test]
fn an_icon_picked_up_near_an_edge_scrolls_only_once_it_has_been_away() {
    // Place 9's icon is 690 px down the visible grid: inside the bottom zone already.
    let surfaces = grid(0.0);
    let (x, y) = icon(9);
    let mut lift = Lift::default();
    lift.arm(key(9), Surface::Grid, (x, y));
    lift.pointer(&at(PointerKind::Move, x, y + 20.0, 0), SLOP, &surfaces, 1.0);
    lift.frame(ms(0), &surfaces, 1.0);
    assert_eq!(lift.frame(ms(16), &surfaces, 1.0).scroll_to, None);

    // Up and out of the zone, then back in.
    finger(&mut lift, &surfaces, x, y - 200.0, 20);
    finger(&mut lift, &surfaces, x, y + 40.0, 30);
    assert!(lift.frame(ms(48), &surfaces, 1.0).scroll_to.is_some());
}

#[test]
fn a_dropped_icon_settles_into_its_new_cell() {
    // ANIM-13: dropped over place 1's label before its wait was over; the shell moved it there.
    let surfaces = grid(0.0);
    let (x, y) = icon(1);
    let mut lift = picked_up(&surfaces, x, y + 170.0);
    lift.frame(ms(0), &surfaces, 1.0);

    let dropped = lift.pointer(
        &at(PointerKind::Up, x + 20.0, y + 160.0, 50),
        SLOP,
        &surfaces,
        1.0,
    );
    assert_eq!(
        dropped,
        Some(LiftEvent::Dropped(DropAt::Grid { key: key(5), to: 1 }))
    );

    let mut moved: Vec<GridItem> = apps(40).to_vec();
    let cell = moved.remove(5);
    moved.insert(1, cell);
    let after = DragSurfaces {
        grid: SurfaceView {
            cells: moved.into(),
            ..grid(0.0).grid
        },
        ..grid(0.0)
    };
    lift.settle(&after, 1.0);
    let state = lift.state();
    assert!(state.settling && state.animating);
    assert_eq!((state.x, state.y), (x + 20.0, y + 160.0));
    assert!(!lift.is_lifted());

    let mut t = 50;
    let mut overshoot: f64 = 0.0;
    while lift.state().key.is_some() {
        t += 8;
        lift.frame(ms(t), &after, 1.0);
        if lift.state().key.is_some() {
            overshoot = overshoot.max(y - lift.state().y);
        }
        assert!(t < 700, "still settling");
    }
    // About 220 ms, overshooting by about 1.5 % (ANIM-02).
    assert!((120..=360).contains(&(t - 50)), "{t}");
    assert!(overshoot <= 160.0 * MAX_OVERSHOOT, "{overshoot}");
}

#[test]
fn a_dropped_icon_lands_at_edit_modes_size_and_place() {
    // ADR 0015: the cell is drawn at 92 % around its middle, so its icon is 4 px lower.
    let surfaces = DragSurfaces {
        scale: 0.92,
        rest_scale: 0.92,
        ..grid(0.0)
    };
    let mut lift = picked_up(&surfaces, 900.0, 900.0);
    lift.pointer(&at(PointerKind::Up, 900.0, 900.0, 0), SLOP, &surfaces, 1.0);
    lift.settle(&surfaces, 1.0);
    for t in (0..=600).step_by(8) {
        lift.frame(ms(t), &surfaces, 1.0);
        if lift.state().key.is_none() {
            break;
        }
    }
    assert_eq!(lift.state(), LiftState::default());

    // Where it lands, just before it does.
    let mut lift = picked_up(&surfaces, 900.0, 900.0);
    lift.pointer(&at(PointerKind::Up, 900.0, 900.0, 0), SLOP, &surfaces, 1.0);
    lift.settle(&surfaces, 1.0);
    let Phase::Settling { x, y, scale, .. } = &lift.phase else {
        panic!("not settling");
    };
    let (cx, cy) = icon(5);
    assert_eq!(x.target, cx);
    assert!((y.target - (cy + (150.0 - 90.0) * 0.08)).abs() < 1e-9);
    assert_eq!(scale.target, 0.92);
}

#[test]
fn with_animations_removed_a_dropped_icon_is_in_its_cell_at_once() {
    // ANIM-05
    let surfaces = grid(0.0);
    let mut lift = picked_up(&surfaces, 300.0, 100.0);
    lift.pointer(&at(PointerKind::Up, 300.0, 100.0, 0), SLOP, &surfaces, 1.0);

    lift.settle(&surfaces, 0.0);

    assert_eq!(lift.state(), LiftState::default());
}

#[test]
fn a_new_finger_lands_a_settling_icon_at_once() {
    let surfaces = grid(0.0);
    let mut lift = picked_up(&surfaces, 900.0, 900.0);
    lift.pointer(&at(PointerKind::Up, 900.0, 900.0, 0), SLOP, &surfaces, 1.0);
    lift.settle(&surfaces, 1.0);
    assert!(lift.state().settling);

    lift.pointer(&at(PointerKind::Down, 0.0, 0.0, 0), SLOP, &surfaces, 1.0);

    assert_eq!(lift.state(), LiftState::default());
}

/// Apps 0 to 39 in the grid, with app 6 replaced by a folder of `count` apps.
fn with_folder(count: usize) -> DragSurfaces {
    let mut cells: Vec<GridItem> = apps(40).to_vec();
    cells[6] = GridItem::Folder {
        id: FolderId(1),
        name: String::new(),
        apps: (0..count).map(|i| app(&format!("in{i}"), "In")).collect(),
    };
    DragSurfaces {
        grid: SurfaceView {
            cells: cells.into(),
            ..grid(0.0).grid
        },
        ..grid(0.0)
    }
}

#[test]
fn a_dropped_app_shrinks_into_its_folders_preview() {
    // ANIM-14: into slot 1 of a folder made from app 6, over 250 ms.
    let surfaces = grid(0.0);
    let (x, y) = icon(6);
    let mut lift = picked_up(&surfaces, x, y);
    lift.pointer(&at(PointerKind::Up, x, y, 500), SLOP, &surfaces, 1.0);

    lift.merge(&with_folder(2), FolderId(1), 1, true, 1.0);

    assert!(lift.is_merging());
    assert_eq!(
        lift.merge_state(),
        MergeState {
            folder: Some(FolderId(1)),
            slot: Some(1),
            formed: true,
            progress: 0.0
        }
    );
    for t in (500..=625).step_by(5) {
        lift.frame(ms(t), &surfaces, 1.0);
    }
    let half = lift.state();
    assert!(half.merging && half.x > x && half.y < y);
    assert!(half.scale < 1.0 && half.alpha == 1.0);
    for t in (630..=760).step_by(5) {
        lift.frame(ms(t), &surfaces, 1.0);
    }
    assert_eq!(lift.state(), LiftState::default());
    assert_eq!(lift.merge_state(), MergeState::default());
}

#[test]
fn an_app_dropped_into_a_full_folder_fades_into_its_icon() {
    let surfaces = grid(0.0);
    let (x, y) = icon(6);
    let mut lift = picked_up(&surfaces, x + 10.0, y);
    lift.pointer(&at(PointerKind::Up, x + 10.0, y, 500), SLOP, &surfaces, 1.0);

    lift.merge(&with_folder(5), FolderId(1), 4, false, 1.0);
    for t in (500..=749).step_by(1) {
        lift.frame(ms(t), &surfaces, 1.0);
    }

    let state = lift.state();
    assert!(state.alpha < 0.01 && (state.x - x).abs() < 0.1);
    assert_eq!(lift.merge_state().slot, None);
}

#[test]
fn with_animations_removed_or_no_folder_the_merge_is_over_at_once() {
    let surfaces = grid(0.0);
    let mut lift = picked_up(&surfaces, 300.0, 300.0);
    lift.merge(&with_folder(2), FolderId(1), 1, true, 0.0);
    assert_eq!(lift.state(), LiftState::default());

    let mut lift = picked_up(&surfaces, 300.0, 300.0);
    lift.merge(&surfaces, FolderId(9), 1, true, 1.0);
    assert_eq!(lift.state(), LiftState::default());
}

/// The grid, with folder 1 at place 6 open as a panel from (100, 500) to (1000, 1100): its
/// grid of 3 apps, 4 columns of 200 px from 150 px in, rows of 300 px, its top 600 px down.
fn folder_open() -> DragSurfaces {
    DragSurfaces {
        folder: Some(SurfaceView {
            layout: Some(GridLayout {
                width_px: 800.0,
                left_px: 150.0,
                view_top_px: 600.0,
                top_px: 0.0,
                bottom_px: 0.0,
                height_px: 300.0,
                ..layout()
            }),
            scroll_px: 0.0,
            cells: (0..3)
                .map(|i| GridItem::App {
                    app: app(&format!("in{i}"), "In"),
                    new: false,
                })
                .collect(),
            rows: 1,
        }),
        folder_bounds: Some(Bounds {
            left: 100.0,
            top: 500.0,
            right: 1000.0,
            bottom: 1100.0,
        }),
        ..with_folder(3)
    }
}

#[test]
fn an_app_moves_within_the_open_folder() {
    // FOLD-05: app in0 to place 2; its icon centre is at (250, 690).
    let surfaces = folder_open();
    let mut lift = Lift::default();
    let in0 = CellKey::App(app_id("in0"));
    lift.arm(in0.clone(), Surface::Folder, (250.0, 690.0));
    lift.pointer(
        &at(PointerKind::Move, 650.0, 870.0, 0),
        SLOP,
        &surfaces,
        1.0,
    );
    lift.frame(ms(0), &surfaces, 1.0);

    assert_eq!(lift.hover(), HoverState::default());
    assert_eq!(
        lift.frame(REORDER_DWELL, &surfaces, 1.0).arrange,
        Some(DropAt::Folder {
            app: app_id("in0"),
            to: 2
        })
    );
    let dropped = lift.pointer(
        &at(PointerKind::Up, 650.0, 870.0, 200),
        SLOP,
        &surfaces,
        1.0,
    );
    assert_eq!(
        dropped,
        Some(LiftEvent::Dropped(DropAt::Folder {
            app: app_id("in0"),
            to: 2
        }))
    );
}

#[test]
fn an_app_dragged_out_of_the_panel_comes_out_where_the_finger_is() {
    // FOLD-05: out past the panel's top, over grid place 1; then it is a grid drag.
    let surfaces = folder_open();
    let mut lift = Lift::default();
    lift.arm(CellKey::App(app_id("in0")), Surface::Folder, (250.0, 690.0));
    lift.pointer(
        &at(PointerKind::Move, 250.0, 650.0, 0),
        SLOP,
        &surfaces,
        1.0,
    );

    let (x, y) = icon(1);
    let left = lift.pointer(
        &at(PointerKind::Move, x, y + 170.0, 10),
        SLOP,
        &surfaces,
        1.0,
    );

    assert_eq!(
        left,
        Some(LiftEvent::LeftFolder {
            app: app_id("in0"),
            to: 1
        })
    );
    assert_eq!(
        lift.held(),
        Some((&CellKey::App(app_id("in0")), Surface::Grid))
    );
    let state = lift.state();
    assert_eq!(
        (state.surface, state.x, state.y),
        (Surface::Grid, x, y + 170.0)
    );
}

#[test]
fn right_to_left_mirrors_the_places() {
    // L10N-02: moving left of place 5 is moving to a later place.
    let mut surfaces = grid(0.0);
    surfaces.rtl = true;
    let mut lift = Lift::default();
    // Place 5 is drawn in the third column from the left.
    let (x, y) = (50.0 + 500.0 + 125.0, 100.0 + 300.0 + 90.0);
    lift.arm(key(5), Surface::Grid, (x, y));
    lift.pointer(
        &at(PointerKind::Move, x - 250.0, y + 170.0, 0),
        SLOP,
        &surfaces,
        1.0,
    );
    lift.frame(ms(0), &surfaces, 1.0);
    assert_eq!(
        lift.frame(REORDER_DWELL, &surfaces, 1.0).arrange,
        Some(DropAt::Grid { key: key(5), to: 6 })
    );
}
