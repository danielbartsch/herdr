use super::*;

fn kinds(rows: Vec<Vec<crate::ui::ResolvedToken>>) -> Vec<crate::ui::ResolvedTokenKind> {
    rows.iter()
        .flatten()
        .map(|token| token.kind.clone())
        .collect()
}

#[test]
fn linked_worktree_branch_uses_the_worktree_glyph() {
    let config = crate::config::SpacesSidebarConfig::default();
    let mut workspace = snapshot().workspaces.remove(0);
    workspace.branch = Some("feature/login".into());
    workspace.custom_label = false;
    workspace.worktree = Some(ClientShellWorktree {
        key: "repo".into(),
        label: "repo".into(),
        is_linked_worktree: true,
    });

    // Top-level worktree space: the branch token carries the worktree glyph.
    let top = kinds(crate::client::shell::render::sidebar::workspace_rows(
        &workspace,
        AgentStatus::Idle,
        false,
        &config,
    ));
    assert!(
        top.contains(&crate::ui::ResolvedTokenKind::Branch(
            "⇱ feature/login".into()
        )),
        "top-level worktree branch should use the worktree glyph; got {top:?}"
    );

    // Indented worktree child shows its branch as the label, still glyphed.
    let child = kinds(crate::client::shell::render::sidebar::workspace_rows(
        &workspace,
        AgentStatus::Idle,
        true,
        &config,
    ));
    assert!(
        child.contains(&crate::ui::ResolvedTokenKind::Workspace(
            "⇱ feature/login".into()
        )),
        "indented worktree label should use the worktree glyph; got {child:?}"
    );
}

#[test]
fn a_non_worktree_space_keeps_the_plain_branch_glyph() {
    let config = crate::config::SpacesSidebarConfig::default();
    let mut workspace = snapshot().workspaces.remove(0);
    workspace.branch = Some("main".into());
    workspace.worktree = None;

    let rows = kinds(crate::client::shell::render::sidebar::workspace_rows(
        &workspace,
        AgentStatus::Idle,
        false,
        &config,
    ));
    assert!(
        rows.contains(&crate::ui::ResolvedTokenKind::Branch("⎇ main".into())),
        "a non-worktree space should keep the plain branch glyph; got {rows:?}"
    );
}
