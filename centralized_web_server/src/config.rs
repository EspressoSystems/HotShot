pub const WEB_SERVER_PORT: u16 = 9000;

pub fn get_proposal_route(view_number: u128) -> String {
    format!("api/proposal/{}", view_number.to_string())
}

pub fn post_proposal_route(view_number: u128) -> String {
    format!("api/proposal/{}", view_number)
}

pub fn get_vote_route(view_number: u128, index: u128) -> String {
    format!("api/votes/{}/{}", view_number, index)
}

pub fn post_vote_route(view_number: u128) -> String {
    format!("api/votes/{}", view_number)
}

pub fn get_transactions_route(index: u128) -> String {
    format!("api/transactions/{}", index)
}

pub fn post_transactions_route() -> String {
    format!("api/transactions")
}
