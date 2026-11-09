//! The library pages beside the grid, each a child of the content stack.

use crate::library::duplicates_view::{DuplicatesView, build_duplicates_view};
use crate::library::folders_view::{FoldersView, build_folders_view};
use crate::library::people_view::{PeopleView, build_people_view};
use crate::library::shared_links_view::{SharedLinksView, build_shared_links_view};
use crate::library::sharing_view::{SharingView, build_sharing_view};
use crate::library::tags_view::{TagsView, build_tags_view};

pub(super) struct Pages {
    pub(super) shared_links: SharedLinksView,
    pub(super) people: PeopleView,
    pub(super) tags: TagsView,
    pub(super) folders: FoldersView,
    pub(super) sharing: SharingView,
    pub(super) duplicates: DuplicatesView,
}

impl Pages {
    pub(super) fn build(stack: &gtk::Stack) -> Self {
        let shared_links = build_shared_links_view();
        stack.add_named(&shared_links.root, Some("shared-links"));
        let people = build_people_view();
        stack.add_named(&people.root, Some("people"));
        let tags = build_tags_view();
        stack.add_named(&tags.page.root, Some("tags"));
        let folders = build_folders_view();
        stack.add_named(&folders.page.root, Some("folders"));
        let sharing = build_sharing_view();
        stack.add_named(&sharing.root, Some("sharing"));
        let duplicates = build_duplicates_view();
        stack.add_named(&duplicates.root, Some("duplicates"));
        Self {
            shared_links,
            people,
            tags,
            folders,
            sharing,
            duplicates,
        }
    }
}
