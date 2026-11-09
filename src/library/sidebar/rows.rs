//! Fixed sidebar destinations and the section each one belongs to.

use crate::i18n::gettext_noop;

pub(super) const GROUPS: [(&str, &[&str]); 5] = [
    ("", &["search"]),
    (
        gettext_noop("Library"),
        &[
            "photos",
            "explore",
            "map",
            "people",
            "favorites",
            "archive",
            "trash",
        ],
    ),
    (
        gettext_noop("Albums & Sharing"),
        &["albums", "shared-albums", "shared-links", "sharing"],
    ),
    (gettext_noop("Browse"), &["tags", "folders"]),
    (
        gettext_noop("Utilities"),
        &[
            "duplicates",
            "large-files",
            "missing-location",
            "unusual-dates",
        ],
    ),
];

/// Fixed sidebar destinations: title, subtitle, icon, and the key the sidebar handler matches.
pub(super) type FixedRow = (&'static str, &'static str, &'static str, &'static str);

pub(super) const FIXED_ROWS: [FixedRow; 18] = [
    fixed_row(
        gettext_noop("Photos"),
        gettext_noop("Timeline of every photo and video"),
        "image-x-generic-symbolic",
        "photos",
    ),
    fixed_row(
        gettext_noop("Search"),
        gettext_noop("Find photos, videos, and text"),
        "system-search-symbolic",
        "search",
    ),
    fixed_row(
        gettext_noop("Explore"),
        gettext_noop("People, places, and things"),
        "view-grid-symbolic",
        "explore",
    ),
    fixed_row(
        gettext_noop("Map"),
        gettext_noop("Photos by where they were taken"),
        "mark-location-symbolic",
        "map",
    ),
    fixed_row(
        gettext_noop("People"),
        gettext_noop("Faces Immich has recognized"),
        "avatar-default-symbolic",
        "people",
    ),
    fixed_row(
        gettext_noop("Albums"),
        gettext_noop("Recent, owned, and shared"),
        "folder-pictures-symbolic",
        "albums",
    ),
    fixed_row(
        gettext_noop("Shared Albums"),
        gettext_noop("Shared with you and by you"),
        "folder-publicshare-symbolic",
        "shared-albums",
    ),
    fixed_row(
        gettext_noop("Shared Links"),
        gettext_noop("Public links you've created"),
        "mimick-share-symbolic",
        "shared-links",
    ),
    fixed_row(
        gettext_noop("Sharing"),
        gettext_noop("Partners who share libraries"),
        "system-users-symbolic",
        "sharing",
    ),
    fixed_row(
        gettext_noop("Favorites"),
        gettext_noop("Photos and videos you've marked"),
        "mimick-heart-filled-symbolic",
        "favorites",
    ),
    fixed_row(
        gettext_noop("Tags"),
        gettext_noop("Browse and organize tags"),
        "mimick-tag-symbolic",
        "tags",
    ),
    fixed_row(
        gettext_noop("Folders"),
        gettext_noop("Browse by folder on the server"),
        "folder-symbolic",
        "folders",
    ),
    fixed_row(
        gettext_noop("Archive"),
        gettext_noop("Hidden from Photos"),
        "mimick-archive-symbolic",
        "archive",
    ),
    fixed_row(
        gettext_noop("Trash"),
        gettext_noop("Restore or permanently delete"),
        "user-trash-symbolic",
        "trash",
    ),
    fixed_row(
        gettext_noop("Duplicates"),
        gettext_noop("Photos Immich found more than once"),
        "edit-copy-symbolic",
        "duplicates",
    ),
    fixed_row(
        gettext_noop("Large Files"),
        gettext_noop("The photos and videos taking the most space"),
        "drive-harddisk-symbolic",
        "large-files",
    ),
    fixed_row(
        gettext_noop("Missing Location"),
        gettext_noop("Photos and videos without a place"),
        "find-location-symbolic",
        "missing-location",
    ),
    fixed_row(
        gettext_noop("Unusual Dates"),
        gettext_noop("Dated in the future, or too long ago"),
        "x-office-calendar-symbolic",
        "unusual-dates",
    ),
];

const fn fixed_row(
    title: &'static str,
    subtitle: &'static str,
    icon: &'static str,
    key: &'static str,
) -> FixedRow {
    (title, subtitle, icon, key)
}
