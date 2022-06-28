use futures::{
    channel::mpsc::{Receiver, Sender},
    FutureExt, SinkExt,
};

use actix_web::{get, post, web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder};
use futures::StreamExt;
use serde::Serialize;

use crate::jkf::JsonKifFormat;

pub async fn server(port: u16) -> anyhow::Result<()> {
    let address = format!("localhost:{}", port);
    eprintln!("Serving rsfm on http://{}", address);

    HttpServer::new(|| App::new().service(index).service(solve))
        .bind(address)?
        .run()
        .await?;
    Ok(())
}

#[get("/{filename:.*}")]
async fn index(req: HttpRequest) -> Result<actix_files::NamedFile, actix_web::Error> {
    let name = req.match_info().query("filename");
    static_file(if name.is_empty() { "index.html" } else { &name })
}

fn static_file(name: &str) -> Result<actix_files::NamedFile, actix_web::Error> {
    let mut path: std::path::PathBuf = ["app", "build"].iter().collect();
    path.push(name);
    let file = actix_files::NamedFile::open(path)?;
    Ok(file)
}

#[derive(Serialize)]
enum SolveResponse {
    Error(String),
    Progress(usize),
    Solved(JsonKifFormat),
}

// Returns line delimited json stream
#[post("/solve")]
async fn solve(body_sfen: String) -> HttpResponse {
    let problem = match crate::sfen::decode_position(&body_sfen) {
        Ok(problem) => problem,
        Err(e) => return HttpResponse::BadRequest().body(e.to_string()),
    };

    let (step_tx, step_rx) = futures::channel::mpsc::unbounded::<usize>();
    let (res_tx, res_rx) = futures::channel::mpsc::unbounded::<SolveResponse>();

    std::thread::spawn(move || {
        let res = match crate::solver::solve_with_progress(step_tx, problem.clone(), None) {
            Ok(solutions) => SolveResponse::Solved(crate::converter::convert(&problem, &solutions)),
            Err(e) => SolveResponse::Error(e.to_string()),
        };
        res_tx.unbounded_send(res).unwrap();
    });

    let stream = step_rx.map(|x| SolveResponse::Progress(x)).chain(res_rx);

    HttpResponse::Ok()
        .content_type("application/json")
        .streaming::<_, String>(
            stream.map(|x| Ok((serde_json::to_string(&x).unwrap() + "\n").into())),
        )
}
