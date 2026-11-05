use crate::api_client::{Activity, ActivityKind, ActivityStatistics, LibraryAlbum, UserSummary};

use super::header_format::count_noun;

mod dialog;
mod feed;
#[cfg(test)]
mod tests;

pub(super) use dialog::header_button;
pub(super) use feed::ActivityFeed;

pub(super) fn has_activity(album: &LibraryAlbum) -> bool {
    album.is_activity_enabled && album.is_shared()
}

fn my_like<'a>(
    activities: &'a [Activity],
    me: &str,
    asset_id: Option<&str>,
) -> Option<&'a Activity> {
    activities.iter().find(|a| {
        a.kind == ActivityKind::Like && a.user.id == me && a.asset_id.as_deref() == asset_id
    })
}

fn can_delete(activity: &Activity, me: &str, owner_id: &str) -> bool {
    !me.is_empty() && (activity.user.id == me || owner_id == me)
}

fn apply_change(activities: &mut Vec<Activity>, removed: Option<&str>, added: Option<Activity>) {
    let replaced = added.as_ref().map(|a| a.id.as_str());
    activities.retain(|a| Some(a.id.as_str()) != removed && Some(a.id.as_str()) != replaced);
    activities.extend(added);
}

fn user_name(user: &UserSummary) -> &str {
    if user.name.is_empty() {
        &user.email
    } else {
        &user.name
    }
}

fn like_text(activity: &Activity) -> &'static str {
    if activity.asset_id.is_some() {
        "Liked this photo"
    } else {
        "Liked the album"
    }
}

fn stats_summary(stats: ActivityStatistics) -> String {
    format!(
        "{} · {}",
        count_noun(stats.likes, "like", "likes"),
        count_noun(stats.comments, "comment", "comments")
    )
}
