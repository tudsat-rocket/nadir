//! Fixed regions of the window: the parts of the frame that are always present and are not part of
//! the user-arrangeable tile tree. Each one owns its persistent state plus whatever panes and
//! widgets it hosts, and mounts itself on the [`egui::Context`] through a single `show`.

mod sidebar;
pub use sidebar::{Sidebar, SidebarAction};

mod status_bar;
pub use status_bar::StatusBar;
