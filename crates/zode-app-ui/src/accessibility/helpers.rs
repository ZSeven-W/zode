use zode_app_model::ComingSoonFeature;

use super::{
    WidgetId, CHATS_NAV_ID, HELP_ID, PULL_REQUESTS_NAV_ID, SCHEDULED_NAV_ID, SITES_NAV_ID,
};

pub(super) const fn coming_soon_focus(feature: ComingSoonFeature) -> Option<WidgetId> {
    Some(match feature {
        ComingSoonFeature::ScheduledTasks => SCHEDULED_NAV_ID,
        ComingSoonFeature::Sites => SITES_NAV_ID,
        ComingSoonFeature::PullRequests => PULL_REQUESTS_NAV_ID,
        ComingSoonFeature::Chats => CHATS_NAV_ID,
        ComingSoonFeature::Help => HELP_ID,
    })
}

pub(super) fn preview_accessibility_excerpt(content: &str) -> String {
    const LIMIT: usize = 2_000;
    let mut chars = content.chars();
    let excerpt = chars.by_ref().take(LIMIT).collect::<String>();
    if chars.next().is_some() {
        format!("{excerpt}…")
    } else {
        excerpt
    }
}
