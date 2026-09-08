use super::*;
use crate::api::schema::AgentStatus;
use crate::config::StatusIndicatorStyle;

#[test]
fn background_status_uses_its_own_glyph_in_the_working_color() {
    let palette = ClientShellConfig::from_config(&Config::default()).palette;

    // The background badge is the same in both indicator styles.
    assert_eq!(
        crate::client::shell::status_icon(AgentStatus::Background, StatusIndicatorStyle::Dots),
        "◯"
    );
    assert_eq!(
        crate::client::shell::status_icon(AgentStatus::Background, StatusIndicatorStyle::Symbols),
        "◯"
    );

    // It reuses the working color, differing only by glyph.
    assert_eq!(
        crate::client::shell::status_color(AgentStatus::Background, &palette),
        palette.yellow
    );
    assert_eq!(
        crate::client::shell::status_color(AgentStatus::Working, &palette),
        palette.yellow
    );

    // It ranks above idle but below an actively working foreground.
    assert!(
        crate::client::shell::status_priority(AgentStatus::Idle)
            < crate::client::shell::status_priority(AgentStatus::Background)
    );
    assert!(
        crate::client::shell::status_priority(AgentStatus::Background)
            < crate::client::shell::status_priority(AgentStatus::Working)
    );
}
