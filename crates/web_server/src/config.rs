/// the default port on which to run the web server
pub const DEFAULT_WEB_SERVER_PORT: u16 = 9000;
/// the default port on which to serve Data availability functionality
pub const DEFAULT_WEB_SERVER_DA_PORT: u16 = 9001;
/// the default port on which to serve View Sync functionality
pub const DEFAULT_WEB_SERVER_VIEW_SYNC_PORT: u16 = 9002;

/// How many views to keep in memory
pub const MAX_VIEWS: usize = 100;
/// How many transactions to keep in memory
pub const MAX_TXNS: usize = 500;
/// How many transactions to return at once
pub const TX_BATCH_SIZE: u64 = 1;

/// get proposal
pub fn get_proposal_route(view_number: u64) -> String {
    format!("api/proposal/{view_number}")
}

/// post proposal
pub fn post_proposal_route(view_number: u64) -> String {
    format!("api/proposal/{view_number}")
}

/// get latest qc
pub fn get_latest_quorum_proposal_route() -> String {
    "api/proposal/latest".to_string()
}

/// get latest view sync proposal
pub fn get_latest_view_sync_proposal_route() -> String {
    "api/view_sync_proposal/latest".to_string()
}

/// get latest certificate
pub fn get_da_certificate_route(view_number: u64) -> String {
    format!("api/certificate/{view_number}")
}

/// post data availability certificate
pub fn post_da_certificate_route(view_number: u64) -> String {
    format!("api/certificate/{view_number}")
}

/// get vote
pub fn get_vote_route(view_number: u64, index: u64) -> String {
    format!("api/votes/{view_number}/{index}")
}

/// post vote
pub fn post_vote_route(view_number: u64) -> String {
    format!("api/votes/{view_number}")
}

/// get vid dispersal
pub fn get_vid_disperse_route(view_number: u64) -> String {
    format!("api/vid_disperse/{view_number}")
}

/// post vid dispersal
pub fn post_vid_disperse_route(view_number: u64) -> String {
    format!("api/vid_disperse/{view_number}")
}

/// get vid vote
pub fn get_vid_vote_route(view_number: u64, index: u64) -> String {
    format!("api/vid_votes/{view_number}/{index}")
}

/// post vid vote
pub fn post_vid_vote_route(view_number: u64) -> String {
    format!("api/vid_votes/{view_number}")
}

/// get vid certificate
pub fn get_vid_certificate_route(view_number: u64) -> String {
    format!("api/vid_certificate/{view_number}")
}

/// post vid certificate
pub fn post_vid_certificate_route(view_number: u64) -> String {
    format!("api/vid_certificate/{view_number}")
}

/// get transactions
pub fn get_transactions_route(index: u64) -> String {
    format!("api/transactions/{index}")
}

/// post transactions
pub fn post_transactions_route() -> String {
    "api/transactions".to_string()
}

/// post stake table
pub fn post_staketable_route() -> String {
    "api/staketable".to_string()
}

/// post view sync proposal
pub fn post_view_sync_proposal_route(view_number: u64) -> String {
    format!("api/view_sync_proposal/{view_number}")
}

/// get view sync proposal
pub fn get_view_sync_proposal_route(view_number: u64, index: u64) -> String {
    format!("api/view_sync_proposal/{view_number}/{index}")
}

/// post view sync vote
pub fn post_view_sync_vote_route(view_number: u64) -> String {
    format!("api/view_sync_vote/{view_number}")
}

/// get view sync vote
pub fn get_view_sync_vote_route(view_number: u64, index: u64) -> String {
    format!("api/view_sync_vote/{view_number}/{index}")
}
