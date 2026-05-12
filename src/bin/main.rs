use loco_rs::cli;
use migration::Migrator;
use taverna::app::App;

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> loco_rs::Result<()> {
    dotenvy::dotenv().ok();
    cli::main::<App, Migrator>().await
}
