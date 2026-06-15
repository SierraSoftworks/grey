pub mod banner;
pub mod cluster_status;
pub mod header;
pub mod history;
// The admin UI reads DOM inputs and performs authenticated mutations, so it is browser-only.
#[cfg(feature = "wasm")]
pub mod incidents_admin;
pub mod incidents_page;
pub mod incidents_timeline;
pub mod markdown;
pub mod probe;
pub mod probe_list;
pub mod status;
pub mod timeline;

pub use banner::{Banner, BannerKind};
pub use cluster_status::ClusterStatus;
pub use header::Header;
pub use history::History;
pub use incidents_page::IncidentsPage;
pub use incidents_timeline::{IncidentBlock, IncidentsSection};
pub use probe::Probe;
pub use probe_list::ProbeList;
pub use timeline::Timeline;

// Re-export UI types from the API library
pub use grey_api::{NoticeLevel, UiNotice};
