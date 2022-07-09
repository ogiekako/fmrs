#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    fmrs::do_main().await
}
