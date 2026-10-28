use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = iplookup::cli::Cli::parse();
    match iplookup::cli::run(cli).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
