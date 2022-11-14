// use tide_disco::{http::StatusCode, Api, App, Error, RequestError};
// use std::io;

use futures::FutureExt;
use tide_disco::error::ServerError;
use tide_disco::Api;
use tide_disco::App;

type State = ();
type Error = ServerError;

#[async_std::main]
async fn main() -> () {
    // let mut app = tide_disco::new();
    // app.at("/orders/shoes").post(order_shoes);
    // app.listen("127.0.0.1:8080").await?;
    let spec =
        toml::from_slice(&std::fs::read("./centralized_web_server/api.toml").unwrap()).unwrap();
    let mut api = Api::<State, Error>::new(spec).unwrap();
    let mut app = App::<State, Error>::with_state(());
    api.get("proposal", |req, state| {
        async move { Ok("Hello, world!") }.boxed()
    })
    .unwrap();

    app.register_module("api", api);
    app.serve("http://localhost:8080").await;

    ()
}
