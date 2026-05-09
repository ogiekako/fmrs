pub mod backward;
pub mod batch_square;
pub mod bench;
pub mod magic;
mod one_way_mate;
mod server;
mod single_king_smoke;
mod smoke_constraints;
pub(crate) mod smoke_features;
mod smoke_persistence;
mod solve;
mod solve_bench;

pub use bench::bench;
pub use one_way_mate::{one_way_mate, OneWayMateGenerator};
pub use server::server;
pub use single_king_smoke::{single_king_smoke, SingleKingSmokeCommand};
pub use solve::solve;
pub use solve_bench::solve_bench;
use url::Url;

pub(crate) fn parse_to_sfen(sfen_or_file_or_url: &str) -> anyhow::Result<String> {
    Ok(match sfen_or_file_or_url {
        x if x.ends_with(".sfen") => std::fs::read_to_string(x)?,
        x if x.starts_with("http") => {
            let url = Url::parse(x)?;
            // path 形式: /fmrs/<sfen> (空白を _ で表現)
            let path = url.path();
            let base = "/fmrs/";
            if path.starts_with(base) && path.len() > base.len() {
                path[base.len()..].replace('_', " ")
            } else {
                // 旧形式: ?sfen=
                url.query_pairs()
                    .find(|(k, _)| k == "sfen")
                    .map(|(_, v)| v.to_string())
                    .ok_or_else(|| anyhow::anyhow!("no sfen query parameter or path"))?
            }
        }
        _ => sfen_or_file_or_url.to_string(),
    })
}
