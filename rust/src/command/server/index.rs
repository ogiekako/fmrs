use actix_cors::Cors;
use actix_web::{get, http::header, post, web, App, HttpRequest, HttpResponse, HttpServer};
use fmrs_core::{
    piece::Color, position::position::PositionAux, validate::decode_and_validate_position,
};
use futures::StreamExt;
use serde::Serialize;

use crate::solver::Algorithm;

pub async fn server(port: u16) -> anyhow::Result<()> {
    let bind_host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let bind_port = std::env::var("PORT")
        .ok()
        .and_then(|x| x.parse::<u16>().ok())
        .unwrap_or(port);
    let address = format!("{}:{}", bind_host, bind_port);
    eprintln!("Serving fmrs api on http://{}", address);

    HttpServer::new(|| {
        App::new()
            .wrap(build_cors())
            .service(fmrs_alive)
            .service(index)
            .service(solve)
    })
    .bind(address)?
    .run()
    .await?;
    Ok(())
}

fn build_cors() -> Cors {
    let origins = allowed_origins();
    let mut cors = Cors::default()
        .allowed_methods(["GET", "POST", "OPTIONS"])
        .allowed_header(header::CONTENT_TYPE)
        .max_age(3600);
    if origins.is_empty() {
        cors = cors.allow_any_origin();
    } else {
        for origin in origins {
            cors = cors.allowed_origin(&origin);
        }
    }
    cors
}

fn allowed_origins() -> Vec<String> {
    std::env::var("FMRS_ALLOWED_ORIGINS")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[get("/")]
async fn index(req: HttpRequest) -> HttpResponse {
    let host = req.connection_info().host().to_string();
    HttpResponse::Ok().json(serde_json::json!({
        "service": "fmrs-api",
        "status": "ok",
        "host": host,
        "endpoints": ["/fmrs_alive", "/solve"],
        "note": "GitHub Pages から /solve を POST してください。"
    }))
}

#[derive(Serialize)]
#[serde(tag = "ty", rename_all = "snake_case")]
enum SolveResponse {
    Error { message: String },
    Progress { step: usize },
    NoSolution,
    Solved { response: ClientSolveResponse },
}

#[derive(Serialize)]
struct ClientSolveResponse {
    redundant: bool,
    solutions: usize,
    kif: String,
    sfen: String,
    from_white: bool,
}

#[derive(serde::Deserialize)]
struct SolveQuery {
    solutions_upto: Option<usize>,
}

// Returns line delimited json stream
#[post("/solve")]
async fn solve(query: web::Query<SolveQuery>, body_sfen: String) -> HttpResponse {
    let problem = match decode_and_validate_position(&body_sfen) {
        Ok(problem) => problem,
        Err(message) => return HttpResponse::BadRequest().body(message),
    };
    let solutions_upto = query.solutions_upto.unwrap_or(10);

    let (step_tx, step_rx) = futures::channel::mpsc::unbounded::<usize>();
    let (res_tx, res_rx) = futures::channel::mpsc::unbounded::<SolveResponse>();

    std::thread::spawn(move || {
        let mut problem = problem;
        let res = match crate::solver::solve_with_progress(
            step_tx,
            problem.clone(),
            Some(solutions_upto),
            Algorithm::Standard,
            None,
        ) {
            Ok(solutions) => {
                if solutions.is_empty() {
                    SolveResponse::NoSolution
                } else {
                    SolveResponse::Solved {
                        response: ClientSolveResponse {
                            redundant: is_redundant(&problem, &solutions),
                            solutions: solutions.len(),
                            kif: fmrs_core::converter::convert_to_kif(&mut problem, &solutions),
                            sfen: body_sfen.clone(),
                            from_white: problem.turn() == Color::WHITE,
                        },
                    }
                }
            }
            Err(e) => SolveResponse::Error {
                message: e.to_string(),
            },
        };
        res_tx.unbounded_send(res).unwrap();
    });

    let stream = step_rx
        .map(|step| SolveResponse::Progress { step })
        .chain(res_rx);

    HttpResponse::Ok()
        .content_type("application/x-ndjson; charset=utf-8")
        .insert_header(("Cache-Control", "no-cache, no-transform"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming::<_, String>(
            stream.map(|x| Ok((serde_json::to_string(&x).unwrap() + "\n").into())),
        )
}

#[get("/fmrs_alive")]
async fn fmrs_alive() -> &'static str {
    "OK"
}

fn is_redundant(problem: &PositionAux, solutions: &[fmrs_core::solve::Solution]) -> bool {
    let Some(first) = solutions.first() else {
        return false;
    };
    let mut position = problem.clone();
    for movement in first {
        position.do_move(movement);
    }
    !position.hands().is_empty(Color::BLACK)
}
