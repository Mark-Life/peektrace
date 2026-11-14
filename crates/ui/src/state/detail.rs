//! The open session's derived view data and the session rail.

use peektrace_protocol::view::{AnalyzedSession, SessionListingView};

use crate::chart::geom::{Growth, growth};
use crate::model::chat::{ChatNode, build_chat_plan, chat_ids_of, chat_node_keys, visible_rows};
use crate::model::list::{self, Filters};
use crate::model::tools::{ToolIndex, index_tools};
use crate::model::turns::{cross_event_index, turn_numbers};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub id: String,
    pub parent: Option<String>,
}

/// An analysis and everything the detail view derives from it.
#[derive(Debug, Clone, PartialEq)]
pub struct Detail {
    pub id: String,
    pub a: AnalyzedSession,
    pub turns: Vec<usize>,
    pub cross: Option<usize>,
    /// Event positions the history shows, in order.
    pub rows: Vec<usize>,
    pub plan: Vec<ChatNode>,
    pub keys: Vec<String>,
    pub ids: Vec<String>,
    pub tools: ToolIndex,
    pub growth: Option<Growth>,
    /// Bumps on every load, so memoised rows rebuild.
    pub rev: u64,
}

impl Detail {
    pub fn new(id: String, a: AnalyzedSession, rev: u64) -> Self {
        let turns = turn_numbers(&a);
        let cross = cross_event_index(&a);
        let rows = visible_rows(&a.events);
        let plan = build_chat_plan(&a.events, &rows, &turns, cross);
        let keys = chat_node_keys(&plan);
        let ids = chat_ids_of(&a.events, &rows);
        let tools = index_tools(&a.events);
        let growth = growth(&a);
        Self {
            id,
            a,
            turns,
            cross,
            rows,
            plan,
            keys,
            ids,
            tools,
            growth,
            rev,
        }
    }
}

/// The session rail: the listing, its order and what the filters keep.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rail {
    pub listing: Option<SessionListingView>,
    /// Header indexes, newest first.
    pub order: Vec<usize>,
    /// Header indexes the filters keep, in order.
    pub rows: Vec<usize>,
    pub filters: Filters,
    pub rev: u64,
}

impl Rail {
    pub fn refilter(&mut self) {
        let headers = self.listing.as_ref().map_or(&[][..], |l| &l.headers);
        self.rows = list::visible_rows(headers, &self.order, &self.filters);
        self.rev += 1;
    }

    pub fn header(&self, i: usize) -> Option<&peektrace_protocol::view::SessionHeader> {
        self.listing.as_ref()?.headers.get(i)
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.listing
            .as_ref()?
            .headers
            .iter()
            .position(|h| h.id == id)
    }
}
