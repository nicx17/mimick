//! Library runtime state used by the built-in asset browser.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use crate::api_client::{
    LibraryAlbum, LibraryAsset, MetadataSearchFilters, ServerAbout, ServerStats,
};

const PAGE_SIZE: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibrarySource {
    AllAssets,
    /// Random sample via `POST /api/search/random`. Used as the "Explore"
    /// sidebar destination — refresh re-rolls; pagination is meaningless.
    Explore,
    Album {
        id: String,
        name: String,
    },
    SmartSearch {
        query: String,
    },
    MetadataSearch {
        query: String,
    },
    OcrSearch {
        query: String,
    },

    AdvancedSearch {
        filters: Box<MetadataSearchFilters>,
    },
    /// Local watched-folder enumeration only (no remote calls).
    LocalAll,
    /// Filename substring filter applied over the local enumeration.
    LocalSearch {
        query: String,
    },
    /// Remote assets overlayed with sync state from local SyncIndex.
    Unified,
    /// Local filename filter applied over a unified view.
    UnifiedSearch {
        query: String,
    },
    /// Local enumeration scoped to the folder linked to a specific album.
    /// Empty if the album has no linked folder.
    AlbumLocal {
        id: String,
        name: String,
    },
    /// Remote album assets overlayed with sync state from the linked folder.
    AlbumUnified {
        id: String,
        name: String,
    },
    /// Assets in the Immich trash, restorable until the server's retention period ends.
    Trash,
    /// Assets marked as favorites.
    Favorites,
    Archive,
    Person {
        id: String,
        name: String,
    },
    Tag {
        id: String,
        name: String,
    },
    Folder {
        path: String,
    },
    Place {
        city: String,
    },
    Thing {
        query: String,
    },
    /// Assets under one cluster on the Map page, fetched by ID.
    MapArea {
        ids: AssetIds,
        label: String,
    },
}

/// Shared asset IDs; debug output shows only the count.
#[derive(Clone, PartialEq, Eq)]
pub struct AssetIds(pub Arc<[String]>);

impl AssetIds {
    /// IDs on 1-based `page`, and whether more pages follow.
    pub fn page(&self, page: u32, size: u32) -> (&[String], bool) {
        let start = (page.saturating_sub(1) as usize).saturating_mul(size as usize);
        let end = start.saturating_add(size as usize).min(self.0.len());
        let ids = self.0.get(start..end).unwrap_or_default();
        (ids, end < self.0.len())
    }
}

impl fmt::Debug for AssetIds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} assets", self.0.len())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryPage {
    Albums { shared_only: bool },
    SharedLinks,
    People,
    Tags { path: Option<String> },
    Folders { path: Option<String> },
    Sharing,
    Search,
    Map,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    Source(LibrarySource),
    Page(LibraryPage),
}

impl Location {
    pub fn is_search(&self) -> bool {
        matches!(self, Location::Source(source) if source.is_search())
    }
}

impl LibrarySource {
    pub fn is_search(&self) -> bool {
        matches!(
            self,
            LibrarySource::SmartSearch { .. }
                | LibrarySource::MetadataSearch { .. }
                | LibrarySource::OcrSearch { .. }
                | LibrarySource::AdvancedSearch { .. }
                | LibrarySource::LocalSearch { .. }
                | LibrarySource::UnifiedSearch { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibrarySortMode {
    NewestFirst,
    OldestFirst,
    Filename,
    FileType,
}

impl LibrarySortMode {
    /// Server-side date order for paged remote sources, or `None` when the
    /// mode is purely client-side (filename/filetype sort over loaded pages).
    pub fn server_order(&self) -> Option<crate::api_client::SortOrder> {
        match self {
            LibrarySortMode::NewestFirst => Some(crate::api_client::SortOrder::Desc),
            LibrarySortMode::OldestFirst => Some(crate::api_client::SortOrder::Asc),
            LibrarySortMode::Filename | LibrarySortMode::FileType => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryLoadState {
    Idle,
    Loading,
    Loaded,
    Empty,
    Error(String),
}

#[derive(Clone, Debug, Default)]
pub struct LibraryStatus {
    pub stats: Option<ServerStats>,
    pub about: Option<ServerAbout>,
}

#[derive(Clone, Debug)]
pub struct LibraryState {
    pub source: LibrarySource,
    pub previous_non_search_source: LibrarySource,
    /// Stack of past sources for back-navigation. Pushed by `navigate_to`,
    /// popped by `navigate_back`. Searches are not pushed (they are ephemeral).
    /// `VecDeque` for O(1) front pops when the 50-entry cap is hit.
    pub nav_history: VecDeque<Location>,
    pub forward_history: Vec<Location>,
    pub location: Location,
    pub sort_mode: LibrarySortMode,
    pub load_state: LibraryLoadState,
    pub selected_asset_id: Option<String>,
    pub albums: Vec<LibraryAlbum>,
    pub assets: Vec<LibraryAsset>,
    pub next_page: u32,
    pub has_more: bool,
    pub page_in_flight: bool,
    pub generation: u64,
    pub status: LibraryStatus,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            source: LibrarySource::AllAssets,
            previous_non_search_source: LibrarySource::AllAssets,
            nav_history: VecDeque::new(),
            forward_history: Vec::new(),
            location: Location::Source(LibrarySource::AllAssets),
            sort_mode: LibrarySortMode::NewestFirst,
            load_state: LibraryLoadState::Idle,
            selected_asset_id: None,
            albums: Vec::new(),
            assets: Vec::new(),
            next_page: 1,
            has_more: true,
            page_in_flight: false,
            generation: 0,
            status: LibraryStatus::default(),
        }
    }
}

impl LibraryState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_albums(&mut self, albums: Vec<LibraryAlbum>) {
        self.albums = albums;
    }

    pub fn load_initial_source(&mut self) -> (u64, LibrarySource, u32) {
        self.switch_source(LibrarySource::AllAssets)
    }

    pub fn switch_source(&mut self, source: LibrarySource) -> (u64, LibrarySource, u32) {
        if !source.is_search() {
            self.previous_non_search_source = source.clone();
        }
        self.source = source.clone();
        self.selected_asset_id = None;
        self.assets.clear();
        self.next_page = 1;
        self.has_more = true;
        self.page_in_flight = true;
        self.generation = self.generation.saturating_add(1);
        self.load_state = LibraryLoadState::Loading;
        (self.generation, source, 1)
    }

    pub fn load_next_page_if_needed(&mut self) -> Option<(u64, LibrarySource, u32)> {
        if self.page_in_flight || !self.has_more {
            return None;
        }

        self.page_in_flight = true;
        Some((self.generation, self.source.clone(), self.next_page))
    }

    pub fn replace_assets(&mut self, generation: u64, items: Vec<LibraryAsset>) -> bool {
        let has_more = items.len() >= PAGE_SIZE;
        self.replace_assets_with_more(generation, items, has_more)
    }

    pub fn append_assets(&mut self, generation: u64, items: Vec<LibraryAsset>) -> bool {
        let has_more = items.len() >= PAGE_SIZE;
        self.append_assets_with_more(generation, items, has_more)
    }

    /// `replace_assets` with an explicit has_more signal — used by search
    /// endpoints that return Immich's `nextPage` field, since the page-size
    /// heuristic is wrong when the server returns short pages due to
    /// post-filtering (visibility, archive, etc.).
    pub fn replace_assets_with_more(
        &mut self,
        generation: u64,
        items: Vec<LibraryAsset>,
        has_more: bool,
    ) -> bool {
        if generation != self.generation {
            return false;
        }

        self.assets = dedup_assets(items);
        self.page_in_flight = false;
        self.next_page = 2;
        self.has_more = has_more;
        self.load_state = if self.assets.is_empty() {
            LibraryLoadState::Empty
        } else {
            LibraryLoadState::Loaded
        };
        self.apply_sort(self.sort_mode.clone());
        true
    }

    pub fn append_assets_with_more(
        &mut self,
        generation: u64,
        items: Vec<LibraryAsset>,
        has_more: bool,
    ) -> bool {
        if generation != self.generation {
            return false;
        }

        let page_len = items.len();
        self.page_in_flight = false;
        self.assets.extend(items);
        self.assets = dedup_assets(std::mem::take(&mut self.assets));

        if page_len > 0 {
            self.next_page = self.next_page.saturating_add(1);
        }
        self.has_more = has_more;
        self.load_state = if self.assets.is_empty() {
            LibraryLoadState::Empty
        } else {
            LibraryLoadState::Loaded
        };
        self.apply_sort(self.sort_mode.clone());
        true
    }

    /// Records the user's preferred sort mode. `LibraryState.assets` is always
    /// stored in insertion (server) order so that the sliding-window FIFO
    /// eviction is well-defined; the listmodel layer applies client-side sort
    /// visually for `Filename`/`FileType`.
    pub fn apply_sort(&mut self, mode: LibrarySortMode) {
        self.sort_mode = mode;
    }

    pub fn clear_search_restore_previous_source(&mut self) -> Option<(u64, LibrarySource, u32)> {
        if self.source.is_search() {
            Some(self.switch_source(self.previous_non_search_source.clone()))
        } else {
            None
        }
    }

    /// Photos as a month timeline: months arrive through `insert_assets_at`, never by paging.
    pub fn enter_timeline(&mut self, generation: u64, empty: bool) -> bool {
        if generation != self.generation {
            return false;
        }
        self.page_in_flight = false;
        self.has_more = false;
        self.load_state = if empty {
            LibraryLoadState::Empty
        } else {
            LibraryLoadState::Loaded
        };
        true
    }

    pub fn insert_assets_at(
        &mut self,
        generation: u64,
        position: usize,
        items: Vec<LibraryAsset>,
    ) -> bool {
        if generation != self.generation {
            return false;
        }
        let position = position.min(self.assets.len());
        self.assets.splice(position..position, items);
        true
    }

    pub fn mark_error(&mut self, generation: u64, message: impl Into<String>) {
        if generation == self.generation {
            self.page_in_flight = false;
            self.load_state = LibraryLoadState::Error(message.into());
        }
    }

    pub fn set_status(&mut self, stats: Option<ServerStats>, about: Option<ServerAbout>) {
        self.status.stats = stats;
        self.status.about = about;
    }
}

fn dedup_assets(items: Vec<LibraryAsset>) -> Vec<LibraryAsset> {
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(items.len());
    for item in items {
        if seen.insert(item.id.clone()) {
            deduped.push(item);
        }
    }
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(id: &str, filename: &str) -> LibraryAsset {
        LibraryAsset {
            id: id.into(),
            filename: filename.into(),
            mime_type: "image/jpeg".into(),
            created_at: format!("2024-01-0{}T00:00:00.000Z", id),
            asset_type: "IMAGE".into(),
            thumbhash: None,
            width: Some(10),
            height: Some(10),
            checksum: None,
            exif_info: None,
            is_favorite: false,
            visibility: String::new(),
            is_edited: false,
            owner_id: String::new(),
            local_date_time: None,
        }
    }

    #[test]
    fn test_switch_source_resets_pagination_and_assets() {
        let mut state = LibraryState::new();
        state.assets.push(asset("1", "a.jpg"));
        state.next_page = 9;
        state.has_more = false;

        let (_, source, page) = state.switch_source(LibrarySource::Album {
            id: "album-1".into(),
            name: "Trips".into(),
        });

        assert!(matches!(source, LibrarySource::Album { .. }));
        assert_eq!(page, 1);
        assert!(state.assets.is_empty());
        assert_eq!(state.next_page, 1);
        assert!(state.has_more);
        assert!(state.page_in_flight);
    }

    #[test]
    fn test_stale_generation_results_are_ignored() {
        let mut state = LibraryState::new();
        let (generation, _, _) = state.load_initial_source();
        let newer_generation = state.switch_source(LibrarySource::MetadataSearch {
            query: "cats".into(),
        });

        assert!(!state.replace_assets(generation, vec![asset("1", "a.jpg")]));
        assert!(state.replace_assets(newer_generation.0, vec![asset("2", "b.jpg")]));
        assert_eq!(state.assets.len(), 1);
        assert_eq!(state.assets[0].id, "2");
    }

    #[test]
    fn test_duplicate_page_requests_are_suppressed() {
        let mut state = LibraryState::new();
        let (_, _, _) = state.load_initial_source();
        assert!(state.load_next_page_if_needed().is_none());
        state.page_in_flight = false;
        assert!(state.load_next_page_if_needed().is_some());
        assert!(state.load_next_page_if_needed().is_none());
    }

    #[test]
    fn test_dedup_drops_duplicates_across_appends() {
        let mut state = LibraryState::new();
        let (generation, _, _) = state.load_initial_source();
        let mk_page = |start: u32| -> Vec<LibraryAsset> {
            (0..50)
                .map(|i| asset(&format!("{}", start + i), "a.jpg"))
                .collect()
        };
        state.append_assets(generation, mk_page(0));
        state.append_assets(generation, mk_page(50));
        // Re-append the second page; ids must not appear twice.
        state.append_assets(generation, mk_page(50));
        let unique: std::collections::HashSet<_> =
            state.assets.iter().map(|a| a.id.clone()).collect();
        assert_eq!(unique.len(), state.assets.len());
        assert_eq!(state.assets.len(), 100);
    }

    #[test]
    fn test_sort_mode_server_order_mapping() {
        assert!(matches!(
            LibrarySortMode::NewestFirst.server_order(),
            Some(crate::api_client::SortOrder::Desc)
        ));
        assert!(matches!(
            LibrarySortMode::OldestFirst.server_order(),
            Some(crate::api_client::SortOrder::Asc)
        ));
        assert!(LibrarySortMode::Filename.server_order().is_none());
        assert!(LibrarySortMode::FileType.server_order().is_none());
    }

    #[test]
    fn test_clear_search_restores_previous_non_search_source() {
        let mut state = LibraryState::new();
        state.switch_source(LibrarySource::Album {
            id: "album-1".into(),
            name: "Trips".into(),
        });
        state.switch_source(LibrarySource::SmartSearch {
            query: "sunset".into(),
        });

        let (_, source, page) = state.clear_search_restore_previous_source().unwrap();
        assert!(matches!(source, LibrarySource::Album { .. }));
        assert_eq!(page, 1);
    }

    #[test]
    fn asset_ids_page_through_in_order() {
        let ids = AssetIds((0..5).map(|i| i.to_string()).collect());
        assert_eq!(ids.page(1, 2), (&ids.0[0..2], true));
        assert_eq!(ids.page(3, 2), (&ids.0[4..5], false));
        assert_eq!(ids.page(4, 2).0, &[] as &[String]);
        assert_eq!(format!("{ids:?}"), "5 assets");
    }

    #[test]
    fn timeline_months_insert_in_place_and_stop_paging() {
        let mut state = LibraryState::new();
        let (generation, _, _) = state.load_initial_source();
        assert!(state.enter_timeline(generation, false));
        assert!(state.load_next_page_if_needed().is_none());
        assert!(state.insert_assets_at(
            generation,
            0,
            vec![asset("1", "a.jpg"), asset("3", "c.jpg")]
        ));
        assert!(state.insert_assets_at(generation, 1, vec![asset("2", "b.jpg")]));
        let ids: Vec<&str> = state.assets.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3"]);
        assert!(!state.insert_assets_at(generation + 1, 0, vec![asset("4", "d.jpg")]));
    }
}
