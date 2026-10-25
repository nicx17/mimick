use super::state::{LibrarySource, LibraryState, Location};

impl LibraryState {
    pub fn navigate_to(&mut self, location: Location) -> bool {
        if self.location == location {
            return false;
        }
        self.forward_history.clear();
        let replace = (self.location.is_search() && location.is_search())
            || (is_map_area(&self.location) && is_map_area(&location));
        if !replace && self.nav_history.back() != Some(&self.location) {
            self.nav_history.push_back(self.location.clone());
            if self.nav_history.len() > 50 {
                self.nav_history.pop_front();
            }
        }
        self.location = location;
        true
    }

    pub fn previous_location(&self) -> Option<&Location> {
        self.nav_history.back()
    }

    pub fn navigate_back(&mut self) -> Option<Location> {
        let previous = self.nav_history.pop_back()?;
        let current = std::mem::replace(&mut self.location, previous.clone());
        self.forward_history.push(current);
        Some(previous)
    }

    pub fn navigate_forward(&mut self) -> Option<Location> {
        let next = self.forward_history.pop()?;
        let current = std::mem::replace(&mut self.location, next.clone());
        self.nav_history.push_back(current);
        Some(next)
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward_history.is_empty()
    }

    pub fn rename_album(&mut self, album_id: &str, new_name: &str) {
        let rename = |source: &mut LibrarySource| {
            if let LibrarySource::Album { id, name }
            | LibrarySource::AlbumLocal { id, name }
            | LibrarySource::AlbumUnified { id, name } = source
                && id == album_id
            {
                *name = new_name.to_string();
            }
        };
        rename(&mut self.source);
        rename(&mut self.previous_non_search_source);
        let locations = std::iter::once(&mut self.location)
            .chain(self.nav_history.iter_mut())
            .chain(self.forward_history.iter_mut());
        for location in locations {
            if let Location::Source(source) = location {
                rename(source);
            }
        }
    }

    pub fn can_go_back(&self) -> bool {
        !self.nav_history.is_empty()
    }
}

fn is_map_area(location: &Location) -> bool {
    matches!(location, Location::Source(LibrarySource::MapArea { .. }))
}

#[cfg(test)]
mod tests {
    use super::super::state::LibraryPage;
    use super::*;

    fn source(source: LibrarySource) -> Location {
        Location::Source(source)
    }

    #[test]
    fn navigate_back_pops_history() {
        let mut state = LibraryState::new();
        state.navigate_to(Location::Page(LibraryPage::Albums { shared_only: false }));
        state.navigate_to(source(LibrarySource::Album {
            id: "a1".into(),
            name: "Trips".into(),
        }));

        assert!(state.can_go_back());
        assert_eq!(
            state.navigate_back(),
            Some(Location::Page(LibraryPage::Albums { shared_only: false }))
        );
        assert_eq!(
            state.navigate_back(),
            Some(source(LibrarySource::AllAssets))
        );
        assert!(!state.can_go_back());
        assert!(state.navigate_back().is_none());
    }

    #[test]
    fn navigating_to_the_current_location_is_a_noop() {
        let mut state = LibraryState::new();
        assert!(!state.navigate_to(source(LibrarySource::AllAssets)));
        assert!(!state.can_go_back());
    }

    #[test]
    fn refining_a_search_replaces_it_but_leaving_it_keeps_it() {
        let mut state = LibraryState::new();
        state.navigate_to(Location::Page(LibraryPage::Search));
        state.navigate_to(source(LibrarySource::SmartSearch {
            query: "sun".into(),
        }));
        state.navigate_to(source(LibrarySource::SmartSearch {
            query: "sunset".into(),
        }));
        state.navigate_to(source(LibrarySource::Trash));

        assert_eq!(
            state.navigate_back(),
            Some(source(LibrarySource::SmartSearch {
                query: "sunset".into()
            }))
        );
        assert_eq!(
            state.navigate_back(),
            Some(Location::Page(LibraryPage::Search))
        );
        assert_eq!(
            state.navigate_back(),
            Some(source(LibrarySource::AllAssets))
        );
    }

    #[test]
    fn forward_returns_after_back_until_a_new_navigation() {
        let mut state = LibraryState::new();
        state.navigate_to(source(LibrarySource::Trash));
        state.navigate_to(source(LibrarySource::Favorites));
        state.navigate_back();
        state.navigate_back();
        assert!(state.can_go_forward());
        assert_eq!(state.navigate_forward(), Some(source(LibrarySource::Trash)));
        assert_eq!(
            state.navigate_back(),
            Some(source(LibrarySource::AllAssets))
        );
        state.navigate_to(source(LibrarySource::Archive));
        assert!(!state.can_go_forward());
        assert_eq!(state.navigate_forward(), None);
    }

    #[test]
    fn tag_drill_down_is_recorded() {
        let mut state = LibraryState::new();
        state.navigate_to(Location::Page(LibraryPage::Tags { path: None }));
        state.navigate_to(Location::Page(LibraryPage::Tags {
            path: Some("Trips".into()),
        }));
        assert_eq!(
            state.navigate_back(),
            Some(Location::Page(LibraryPage::Tags { path: None }))
        );
    }

    #[test]
    fn rename_album_updates_current_view_and_history() {
        let album = |name: &str| {
            source(LibrarySource::Album {
                id: "a1".into(),
                name: name.into(),
            })
        };
        let mut state = LibraryState::new();
        state.navigate_to(album("Trips"));
        state.switch_source(LibrarySource::Album {
            id: "a1".into(),
            name: "Trips".into(),
        });
        state.navigate_to(source(LibrarySource::Trash));
        state.rename_album("a1", "Holidays");
        assert_eq!(state.navigate_back(), Some(album("Holidays")));
        assert_eq!(
            state.source,
            LibrarySource::Album {
                id: "a1".into(),
                name: "Holidays".into()
            }
        );
    }

    #[test]
    fn switching_map_areas_replaces_the_area_and_keeps_the_map() {
        let area = |label: &str| {
            source(LibrarySource::MapArea {
                ids: super::super::state::AssetIds(["a".to_string()].into()),
                label: label.into(),
            })
        };
        let mut state = LibraryState::new();
        state.navigate_to(Location::Page(LibraryPage::Map));
        state.navigate_to(area("Lisbon"));
        state.navigate_to(area("Porto"));
        assert_eq!(
            state.previous_location(),
            Some(&Location::Page(LibraryPage::Map))
        );
        assert_eq!(
            state.navigate_back(),
            Some(Location::Page(LibraryPage::Map))
        );
    }
}
