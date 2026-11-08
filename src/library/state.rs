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
    Partner {
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
    /// One of Immich's memories ("1 year ago"), from the row above Photos.
    Memory {
        id: String,
        title: String,
    },
    /// Smart search with one photo as the query ("View Similar Photos").
    Similar {
        asset_id: String,
        filename: String,
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

/// An album whose photos the Map page shows instead of the whole library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapAlbum {
    pub id: String,
    pub name: String,
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
    Map { album: Option<MapAlbum> },
    Duplicates,
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

    pub fn set_album_order(&mut self, album_id: &str, order: crate::api_client::SortOrder) {
        if let Some(album) = self.albums.iter_mut().find(|album| album.id == album_id) {
            album.order = Some(order);
        }
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
        self.append_unique(items);

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

    fn append_unique(&mut self, items: Vec<LibraryAsset>) {
        let keep: Vec<bool> = {
            let mut seen: std::collections::HashSet<&str> =
                self.assets.iter().map(|asset| asset.id.as_str()).collect();
            items
                .iter()
                .map(|item| seen.insert(item.id.as_str()))
                .collect()
        };
        self.assets.extend(
            items
                .into_iter()
                .zip(keep)
                .filter_map(|(item, keep)| keep.then_some(item)),
        );
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
mod tests;
