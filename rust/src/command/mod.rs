pub mod backward;
pub mod batch_square;
pub mod bench;
mod one_way_mate;
mod server;
mod solve;

pub use bench::bench;
pub use one_way_mate::{one_way_mate, solve::one_way_mate_steps, OneWayMateGenerator};
pub use server::server;
pub use solve::solve;
use url::Url;

fn parse_to_sfen(sfen_or_file_or_url: &str) -> anyhow::Result<String> {
    Ok(match sfen_or_file_or_url {
        x if x.ends_with(".sfen") => std::fs::read_to_string(x)?,
        x if x.starts_with("http") => {
            let url = Url::parse(x)?;
            url.query_pairs()
                .find(|(k, _)| k == "sfen")
                .map(|(_, v)| v.to_string())
                .ok_or_else(|| anyhow::anyhow!("no sfen query parameter"))?
        }
        _ => sfen_or_file_or_url.to_string(),
    })
}
