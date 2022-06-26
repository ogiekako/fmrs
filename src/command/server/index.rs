use actix_web::{get, post, App, HttpResponse, HttpServer, Responder};
use serde::Serialize;

pub async fn server(port: u16) -> anyhow::Result<()> {
    let address = format!("localhost:{}", port);
    eprintln!("Serving rsfm on http://{}", address);

    HttpServer::new(|| App::new().service(index).service(solve))
        .bind(address)?
        .run()
        .await?;
    Ok(())
}

#[get("/")]
async fn index() -> impl Responder {
    "POST /solve <sfen>: run solver"
}

#[derive(Serialize)]
enum SolveResponse {
    Progress(usize),
    Solved(Vec<String>),
}

// Returns line delimited json stream
#[post("/solve")]
async fn solve(body: String) -> impl Responder {
    HttpResponse::Ok().body(body)
}
