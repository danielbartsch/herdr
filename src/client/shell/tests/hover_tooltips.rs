use super::*;

/// Full text carried by all collected hover tooltips, concatenated.
fn tooltip_text(state: &ClientShellState) -> String {
    state
        .hits
        .hover_tooltips
        .iter()
        .flat_map(|tooltip| tooltip.spans.iter().map(|(text, _)| text.clone()))
        .collect()
}

#[test]
fn hovering_a_truncated_space_reveals_its_full_name() {
    const LONG: &str = "an-extremely-long-workspace-name-that-will-not-fit-the-sidebar";
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = LONG.into();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("compose");

    // No tooltip until the mouse rests on the entry.
    assert!(state.hits.hover_tooltips.is_empty());

    let rect = state.hits.workspaces[0].rect;
    state.last_mouse_pos = Some((rect.x + 1, rect.y));
    state.compose(106, 20).expect("compose with hover");

    assert!(
        tooltip_text(&state).contains(LONG),
        "hovering a truncated space should show its full name; got {:?}",
        tooltip_text(&state)
    );
}

#[test]
fn hovering_a_space_that_fits_shows_no_tooltip() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("compose");

    let rect = state.hits.workspaces[0].rect;
    state.last_mouse_pos = Some((rect.x + 1, rect.y));
    state.compose(106, 20).expect("compose with hover");

    assert!(
        state.hits.hover_tooltips.is_empty(),
        "a short label that fits should not produce a tooltip; got {:?}",
        tooltip_text(&state)
    );
}

#[test]
fn moving_off_a_space_clears_its_tooltip() {
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = "an-extremely-long-workspace-name-that-truncates".into();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("compose");

    let rect = state.hits.workspaces[0].rect;
    state.last_mouse_pos = Some((rect.x + 1, rect.y));
    state.compose(106, 20).expect("compose with hover");
    assert!(!state.hits.hover_tooltips.is_empty());

    // Move into the pane area, away from any sidebar entry.
    state.last_mouse_pos = Some((state.hits.panes[0].rect.x + 1, rect.y));
    state.compose(106, 20).expect("compose after moving away");
    assert!(
        state.hits.hover_tooltips.is_empty(),
        "moving off the entry should clear its tooltip"
    );
}

#[test]
fn a_tooltip_never_paints_while_an_overlay_is_open() {
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = "an-extremely-long-workspace-name-that-truncates".into();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("compose");
    let rect = state.hits.workspaces[0].rect;
    state.last_mouse_pos = Some((rect.x + 1, rect.y));

    state.overlay = Some(ClientShellOverlay::GlobalMenu(ClientGlobalMenuOverlay {
        highlighted: 0,
    }));
    state.compose(106, 20).expect("compose with overlay");
    assert!(
        state.hits.hover_tooltips.is_empty(),
        "an open overlay owns the surface, so no sidebar tooltip should be collected"
    );
}

#[test]
fn moving_between_sidebar_entries_forces_a_repaint() {
    let mut initial = snapshot();
    let template = initial.workspaces[0].clone();
    initial.workspaces = (1..=3)
        .map(|number| ClientShellWorkspace {
            workspace_id: format!("ws_{number}"),
            number,
            label: format!("space-number-{number}"),
            focused: number == 1,
            ..template.clone()
        })
        .collect();
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(initial));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("compose");

    let first = state.hits.workspaces[0].rect;
    let second = state.hits.workspaces[1].rect;

    let moved = |column: u16, row: u16| {
        RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        })
    };

    // First move onto an entry: hovered target changes from nothing to it.
    let outcome = state.handle_raw_events(vec![moved(first.x + 1, first.y)]);
    assert!(outcome.repaint, "entering an entry should repaint");

    // Staying on the same entry is render-neutral.
    let outcome = state.handle_raw_events(vec![moved(first.x + 2, first.y)]);
    assert!(
        !outcome.repaint,
        "moving within one entry should not repaint"
    );

    // Crossing to a different entry repaints again.
    let outcome = state.handle_raw_events(vec![moved(second.x + 1, second.y)]);
    assert!(outcome.repaint, "crossing to another entry should repaint");
}

#[test]
fn collapsed_sidebar_hover_reveals_the_hidden_space_details() {
    let mut snapshot = snapshot();
    snapshot.workspaces[0].label = "my-project".into();
    snapshot.workspaces[0].branch = Some("feature/login".into());
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.sidebar_collapsed = true;
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("compose collapsed");

    let rect = state.hits.workspaces[0].rect;
    state.last_mouse_pos = Some((rect.x, rect.y));
    state
        .compose(106, 20)
        .expect("compose collapsed with hover");

    let text = tooltip_text(&state);
    assert!(
        text.contains("my-project"),
        "collapsed hover should reveal the space name; got {text:?}"
    );
}
