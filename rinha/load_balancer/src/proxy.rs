use std::str::FromStr;

use axum::{
    extract::{Request, State},
    http::{
        uri::{Authority, Scheme},
        StatusCode, Uri,
    },
    response::IntoResponse,
};

use crate::model::AppState;

pub async fn main(
    State(AppState {
        load_balancer,
        http_client,
    }): State<AppState>,
    mut req: Request,
) -> impl IntoResponse {
    let addr = load_balancer.next_server(&req);

    *req.uri_mut() = {
        let uri = req.uri();
        let mut parts = uri.clone().into_parts();

        parts.authority = Authority::from_str(addr.as_str()).ok();
        parts.scheme = Some(Scheme::HTTP);

        Uri::from_parts(parts).unwrap()
    };

    match http_client.request(req).await {
        Ok(res) => Ok(res),
        Err(_) => Err(StatusCode::BAD_GATEWAY),
    }
}
