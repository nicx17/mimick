//! Ctrl+A selects everything in the view, not only what has been scrolled to: Photos first
//! loads its missing months, and paged views (albums, people, search) their remaining pages.

use std::rc::Rc;
use std::time::Duration;

use gettextrs::gettext;
use gtk::prelude::*;

use super::LibraryWindowUi;
use super::source_loading::{apply_page_result, fetch_source_page};
use super::timeline;

/// How often to check whether a page that scrolling asked for has arrived.
const IN_FLIGHT_POLL: Duration = Duration::from_millis(100);

pub(super) fn start(ui: &Rc<LibraryWindowUi>) {
    ui.select_toggle.set_active(true);
    let (generation, has_more) = {
        let state = ui.ctx.library_state.lock();
        (state.generation, state.has_more)
    };
    let on_timeline = timeline::is_active(ui);
    if !on_timeline && !has_more {
        ui.grid.selection.select_all();
        return;
    }
    let loading = libadwaita::Toast::builder()
        .title(gettext("Loading every photo to select…"))
        .timeout(0)
        .build();
    ui.toasts.add_toast(loading.clone());
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let complete = if on_timeline {
            timeline::load_all(&ui).await
        } else {
            load_remaining_pages(&ui, generation).await
        };
        loading.dismiss();
        // Leaving the view cancels: its photos are gone from the grid.
        if ui.ctx.library_state.lock().generation != generation {
            return;
        }
        ui.grid.selection.select_all();
        if !complete {
            ui.toasts.add_toast(libadwaita::Toast::new(&gettext(
                "Some photos couldn't be loaded, so not all are selected",
            )));
        }
    });
}

/// Fetch the pages still to come, one at a time. False if one failed or the view changed.
async fn load_remaining_pages(ui: &Rc<LibraryWindowUi>, generation: u64) -> bool {
    loop {
        let next = {
            let mut state = ui.ctx.library_state.lock();
            if state.generation != generation {
                return false;
            }
            if !state.has_more {
                return true;
            }
            state.load_next_page_if_needed()
        };
        let Some((generation, source, page)) = next else {
            // A page scrolling asked for is on its way; it is applied without us.
            glib::timeout_future(IN_FLIGHT_POLL).await;
            continue;
        };
        let order = ui.ctx.library_state.lock().sort_mode.server_order();
        let result = fetch_source_page(ui, source, page, order).await;
        let failed = result.is_err();
        apply_page_result(ui, generation, true, result);
        if failed {
            return false;
        }
    }
}
