//! "12 owned · 5 shared · 7 not shared" under the Albums page's title.

use crate::i18n::fill;
use gettextrs::gettext;
use std::sync::Arc;

use gtk::prelude::*;

use crate::api_client::AlbumStatistics;
use crate::app_context::AppContext;

use super::AlbumsViewParts;

pub(super) fn label() -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .css_classes(["dim-label"])
        .visible(false)
        .build()
}

/// Ask Immich for the counts; the line stays hidden if it can't say.
pub fn refresh_stats(parts: &AlbumsViewParts, ctx: Arc<AppContext>) {
    let label = parts.stats.clone();
    glib::MainContext::default().spawn_local(async move {
        match ctx.api_client.fetch_album_statistics().await {
            Ok(stats) => {
                label.set_label(&stats_text(stats));
                label.set_visible(true);
            }
            Err(err) => log::debug!("Album statistics unavailable: {err}"),
        }
    });
}

/// Numbers come first, so the words don't change with the count.
fn stats_text(stats: AlbumStatistics) -> String {
    fill(
        gettext("{owned} owned · {shared} shared · {not_shared} not shared"),
        &[
            ("owned", &stats.owned.to_string()),
            ("shared", &stats.shared.to_string()),
            ("not_shared", &stats.not_shared.to_string()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_read_as_one_line() {
        let stats = AlbumStatistics {
            owned: 12,
            shared: 5,
            not_shared: 7,
        };
        assert_eq!(stats_text(stats), "12 owned · 5 shared · 7 not shared");
    }
}
