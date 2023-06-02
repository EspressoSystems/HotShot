pub const DEFAULT_WEB_SERVER_PORT: u16 = 9000;
pub const GENERAL_CONSENSUS_API_PATH: &str = "api";
pub const DA_API_PATH: &str = "da";

fn get_api_path(is_da: bool) -> &'static str {
    if is_da {
        DA_API_PATH
    } else {
        GENERAL_CONSENSUS_API_PATH
    }
}

pub fn get_proposal_route(view_number: u64, is_da: bool) -> String {
    let api_path = get_api_path(is_da);
    format!("{api_path}/proposal/{view_number}")
}

pub fn post_proposal_route(view_number: u64, is_da: bool) -> String {
    let api_path = get_api_path(is_da);

    format!("{api_path}/proposal/{view_number}")
}

pub fn get_vote_route(view_number: u64, index: u64, is_da: bool) -> String {
    let api_path = get_api_path(is_da);

    format!("{api_path}/votes/{view_number}/{index}")
}

pub fn post_vote_route(view_number: u64, is_da: bool) -> String {
    let api_path = get_api_path(is_da);

    format!("{api_path}/votes/{view_number}")
}

pub fn get_transactions_route(index: u64, is_da: bool) -> String {
    let api_path = get_api_path(is_da);

    format!("{api_path}/transactions/{index}")
}

pub fn post_transactions_route(is_da: bool) -> String {
    let api_path = get_api_path(is_da);

    format!("{api_path}/transactions")
}

pub fn post_staketable_route(is_da: bool) -> String {
    let api_path = get_api_path(is_da);

    format!("{api_path}/staketable")
}
