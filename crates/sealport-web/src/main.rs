use std::{env, net::SocketAddr};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = env::var("SEALPORT_WEB_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
        .parse::<SocketAddr>()?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("serving sealport.cc homepage on http://{addr}");
    axum::serve(listener, sealport_web::app()).await?;

    Ok(())
}
