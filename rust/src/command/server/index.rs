use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use actix_cors::Cors;
use actix_web::{get, http::header, post, web, App, HttpRequest, HttpResponse, HttpServer};
use fmrs_core::{
    piece::Color,
    position::position::PositionAux,
    shtsume::{pv_to_movements, FmrsPos, SearchConfig, Solver as FutsuuSolver},
    validate::{decode_and_validate_futsuu_position, decode_and_validate_position},
};
use futures::StreamExt;
use serde::Serialize;

use crate::solver::{Algorithm, ProgressReceiverDropped};

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
            .service(solve_futsuu)
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
        "endpoints": ["/fmrs_alive", "/solve", "/solve_futsuu"],
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
            Err(e) if e.is::<ProgressReceiverDropped>() => {
                // クライアントが切断した (キャンセル・タブを閉じた等)。返す先はない。
                eprintln!("solve aborted: client disconnected");
                return;
            }
            Err(e) => SolveResponse::Error {
                message: e.to_string(),
            },
        };
        // 最後の手の直後に切断された場合は受け手がいないが、それで構わない。
        let _ = res_tx.unbounded_send(res);
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

#[derive(Serialize)]
#[serde(tag = "ty", rename_all = "snake_case")]
enum FutsuuSolveResponse {
    Error { message: String },
    Progress { nodes: u64 },
    NoSolution { nodes: u64 },
    Suspended { nodes: u64 },
    Solved { response: FutsuuClientSolveResponse },
}

#[derive(Serialize)]
struct FutsuuClientSolveResponse {
    steps: u16,
    redundant: bool,
    nodes: u64,
    kif: String,
    sfen: String,
}

#[derive(serde::Deserialize)]
struct SolveFutsuuQuery {
    simple_mate: Option<bool>,
}

fn futsuu_server_config(simple_mate: bool) -> SearchConfig {
    let hash_mb = std::env::var("FMRS_FUTSUU_HASH_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(512)
        .clamp(32, 4096);
    let default_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(8);
    let threads = std::env::var("FMRS_FUTSUU_THREADS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(default_threads)
        .max(1);
    SearchConfig {
        hash_mb,
        threads,
        simple_mate,
        use_ni_cache: true,
        use_ml_ordering: true,
        ..Default::default()
    }
}

#[post("/solve_futsuu")]
async fn solve_futsuu(query: web::Query<SolveFutsuuQuery>, body_sfen: String) -> HttpResponse {
    let problem = match decode_and_validate_futsuu_position(&body_sfen) {
        Ok(problem) => problem,
        Err(message) => return HttpResponse::BadRequest().body(message),
    };
    let simple_mate = query.simple_mate.unwrap_or(false);

    let (prog_tx, prog_rx) = futures::channel::mpsc::unbounded::<u64>();
    let (res_tx, res_rx) = futures::channel::mpsc::unbounded::<FutsuuSolveResponse>();

    let abort_flag = Arc::new(AtomicBool::new(false));
    let abort_for_cb = Arc::clone(&abort_flag);

    std::thread::spawn(move || {
        let mut problem = problem;
        let root = FmrsPos::from_pos_aux_root(problem.clone());
        let mut solver = FutsuuSolver::new(futsuu_server_config(simple_mate));
        solver.set_abort_flag(Some(abort_flag));
        solver.set_progress_callback(Some(Arc::new(move |nodes: u64| {
            if prog_tx.unbounded_send(nodes).is_err() {
                abort_for_cb.store(true, Ordering::Relaxed);
            }
        })));

        let res = solver.solve(&root);
        let event = if res.error {
            FutsuuSolveResponse::Error {
                message: "内部エラーにより探索を継続できませんでした。".to_string(),
            }
        } else {
            match res.mate {
                Some(true) => {
                    let movements = pv_to_movements(&res.pv);
                    let kif = fmrs_core::converter::convert_to_kif(
                        &mut problem,
                        std::slice::from_ref(&movements),
                    );
                    FutsuuSolveResponse::Solved {
                        response: FutsuuClientSolveResponse {
                            steps: res.mate_steps,
                            redundant: res.redundant,
                            nodes: res.nodes,
                            kif,
                            sfen: problem.sfen(),
                        },
                    }
                }
                Some(false) => FutsuuSolveResponse::NoSolution { nodes: res.nodes },
                None => FutsuuSolveResponse::Suspended { nodes: res.nodes },
            }
        };
        let _ = res_tx.unbounded_send(event);
    });

    let stream = prog_rx
        .map(|nodes| FutsuuSolveResponse::Progress { nodes })
        .chain(res_rx);

    HttpResponse::Ok()
        .content_type("application/x-ndjson; charset=utf-8")
        .insert_header(("Cache-Control", "no-cache, no-transform"))
        .insert_header(("X-Accel-Buffering", "no"))
        .streaming::<_, String>(
            stream.map(|x| Ok((serde_json::to_string(&x).unwrap() + "\n").into())),
        )
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
