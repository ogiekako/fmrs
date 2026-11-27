use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use actix_cors::Cors;
use actix_web::{get, http::header, post, web, App, HttpRequest, HttpResponse, HttpServer};
use fmrs_core::{
    futsuu::{
        perfection::{
            format_futsuu_move_ja, ConventionalVerifier, FlawKind, PerfectionConfig,
            PerfectionStatus,
        },
        pv_to_movements, FmrsPos, SearchConfig, Solver as FutsuuSolver,
    },
    piece::Color,
    position::position::PositionAux,
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
struct FutsuuPerfectionInfo {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    shortcut_ply: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shortcut_total_steps: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shortcut_move_kif: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    intended_move_kif: Option<String>,
    minor_flaw_count: u16,
}

#[derive(Serialize)]
struct FutsuuClientSolveResponse {
    steps: u16,
    redundant: bool,
    nodes: u64,
    kif: String,
    sfen: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    perfection: Option<FutsuuPerfectionInfo>,
}

#[derive(serde::Deserialize, Default)]
struct SolveFutsuuQuery {
    #[serde(default)]
    check_perfection: bool,
}

fn futsuu_server_config() -> SearchConfig {
    let hash_mb = std::env::var("FMRS_FUTSUU_HASH_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2048)
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
    let time_limit_secs = match std::env::var("FMRS_FUTSUU_TIME_LIMIT_SECS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
    {
        Some(secs) if secs <= 0.0 => None,
        Some(secs) => Some(secs),
        None => Some(600.0),
    };
    SearchConfig {
        hash_mb,
        threads,
        time_limit_secs,
        simple_mate: false,
        use_ni_cache: true,
        use_ml_ordering: true,
        ..Default::default()
    }
}

#[post("/solve_futsuu")]
async fn solve_futsuu(
    query: web::Query<SolveFutsuuQuery>,
    body_sfen: String,
) -> HttpResponse {
    let problem = match decode_and_validate_futsuu_position(&body_sfen) {
        Ok(problem) => problem,
        Err(message) => return HttpResponse::BadRequest().body(message),
    };
    let check_perfection = query.check_perfection;

    let (prog_tx, prog_rx) = futures::channel::mpsc::unbounded::<u64>();
    let (res_tx, res_rx) = futures::channel::mpsc::unbounded::<FutsuuSolveResponse>();

    let abort_flag = Arc::new(AtomicBool::new(false));
    let abort_for_cb = Arc::clone(&abort_flag);

    std::thread::spawn(move || {
        let t0 = std::time::Instant::now();
        let mut problem = problem;
        let root = FmrsPos::from_pos_aux_root(problem.clone());
        let srv_cfg = futsuu_server_config();
        let total_time_limit = srv_cfg.time_limit_secs;
        let verify_hash_mb = (srv_cfg.hash_mb / 8).clamp(32, 128);
        let mut solver = FutsuuSolver::new(srv_cfg);
        solver.set_abort_flag(Some(Arc::clone(&abort_flag)));
        let progress_cb: Arc<dyn Fn(u64) + Send + Sync> = Arc::new(move |nodes: u64| {
            if prog_tx.unbounded_send(nodes).is_err() {
                abort_for_cb.store(true, Ordering::Relaxed);
            }
        });
        solver.set_progress_callback(Some(Arc::clone(&progress_cb)));

        let res = if check_perfection {
            solver.solve_unbounded_pn(&root)
        } else {
            solver.solve(&root)
        };
        let event = if res.error {
            FutsuuSolveResponse::Error {
                message: "内部エラーにより探索を継続できませんでした。".to_string(),
            }
        } else {
            match res.mate {
                Some(true) => {
                    if !check_perfection {
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
                                perfection: None,
                            },
                        }
                    } else if res.redundant {
                        let movements = pv_to_movements(&res.pv);
                        let kif = fmrs_core::converter::convert_to_kif(
                            &mut problem,
                            std::slice::from_ref(&movements),
                        );
                        FutsuuSolveResponse::Solved {
                            response: FutsuuClientSolveResponse {
                                steps: res.mate_steps,
                                redundant: true,
                                nodes: res.nodes,
                                kif,
                                sfen: problem.sfen(),
                                perfection: Some(FutsuuPerfectionInfo {
                                    status: "redundant",
                                    shortcut_ply: None,
                                    shortcut_total_steps: None,
                                    shortcut_move_kif: None,
                                    intended_move_kif: None,
                                    minor_flaw_count: 0,
                                }),
                            },
                        }
                    } else {
                        drop(solver);
                        let rem_secs = total_time_limit
                            .map(|lim| (lim - t0.elapsed().as_secs_f64()).max(0.5));
                        let mut verifier = ConventionalVerifier::new(PerfectionConfig {
                            hash_mb: verify_hash_mb,
                            solve_node_limit: 10_000_000,
                            check_node_limit_per_move: 500_000,
                            search_level: 1,
                            fail_fast: true,
                            endgame_transposition_rem_sh: 5,
                        });
                        verifier.set_time_limit_secs(rem_secs);
                        verifier.set_abort_flag(Some(abort_flag));
                        verifier.set_progress_callback(Some(progress_cb));

                        let orig_pv = res.pv.clone();
                        let orig_steps = res.mate_steps;
                        let rep = verifier.verify_solved(&root, res);
                        let total_nodes = rep.solve_nodes + rep.check_nodes;

                        if matches!(rep.status, PerfectionStatus::NoMate) {
                            FutsuuSolveResponse::NoSolution { nodes: total_nodes }
                        } else {
                            let final_pv = if !rep.pv.is_empty() { rep.pv } else { orig_pv };
                            let final_steps = if rep.mate_steps > 0 {
                                rep.mate_steps
                            } else {
                                orig_steps
                            };
                            let mut final_redundant = false;
                            let main_movements = pv_to_movements(&final_pv);
                            let mut solutions = vec![main_movements.clone()];

                            let perf_info = match rep.status {
                                PerfectionStatus::Perfect => FutsuuPerfectionInfo {
                                    status: "perfect",
                                    shortcut_ply: None,
                                    shortcut_total_steps: None,
                                    shortcut_move_kif: None,
                                    intended_move_kif: None,
                                    minor_flaw_count: rep.minor_flaw_count,
                                },
                                PerfectionStatus::Shortcut {
                                    first_ply,
                                    alt_move,
                                    intended_move,
                                    kind,
                                } => {
                                    let mut pos_at_ply = problem.clone();
                                    for m in main_movements.iter().take(first_ply as usize) {
                                        pos_at_ply.do_move(m);
                                    }
                                    let sc_move_kif = format_futsuu_move_ja(&pos_at_ply, alt_move);
                                    let int_move_kif =
                                        format_futsuu_move_ja(&pos_at_ply, intended_move);
                                    let sc_total = if !rep.shortcut_pv.is_empty() {
                                        let sc_movements = pv_to_movements(&rep.shortcut_pv);
                                        let len = sc_movements.len() as u16;
                                        solutions.push(sc_movements);
                                        Some(len)
                                    } else {
                                        None
                                    };
                                    FutsuuPerfectionInfo {
                                        status: match kind {
                                            FlawKind::EarlyShortcut => "early_shortcut",
                                            FlawKind::EqualLengthShortcut => "equal_shortcut",
                                        },
                                        shortcut_ply: Some(first_ply + 1),
                                        shortcut_total_steps: sc_total,
                                        shortcut_move_kif: Some(sc_move_kif),
                                        intended_move_kif: Some(int_move_kif),
                                        minor_flaw_count: rep.minor_flaw_count,
                                    }
                                }
                                PerfectionStatus::RedundantHand => {
                                    final_redundant = true;
                                    FutsuuPerfectionInfo {
                                        status: "redundant",
                                        shortcut_ply: None,
                                        shortcut_total_steps: None,
                                        shortcut_move_kif: None,
                                        intended_move_kif: None,
                                        minor_flaw_count: 0,
                                    }
                                }
                                PerfectionStatus::Timeout | PerfectionStatus::NoMate => {
                                    FutsuuPerfectionInfo {
                                        status: "timeout",
                                        shortcut_ply: None,
                                        shortcut_total_steps: None,
                                        shortcut_move_kif: None,
                                        intended_move_kif: None,
                                        minor_flaw_count: rep.minor_flaw_count,
                                    }
                                }
                            };

                            let kif =
                                fmrs_core::converter::convert_to_kif(&mut problem, &solutions);
                            FutsuuSolveResponse::Solved {
                                response: FutsuuClientSolveResponse {
                                    steps: final_steps,
                                    redundant: final_redundant,
                                    nodes: total_nodes,
                                    kif,
                                    sfen: problem.sfen(),
                                    perfection: Some(perf_info),
                                },
                            }
                        }
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
