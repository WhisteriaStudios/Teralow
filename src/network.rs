use std::error::Error;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

type NetworkResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

// Temp protocol markers for conenction test
// These aren't authentication or encryption
const CLIENT_HELLO: &[u8; 8] = b"TERALOW1";
const HOST_HELLO: &[u8; 8] = b"TL_HELLO";

pub async fn start_host(address: &str) -> NetworkResult<()> {
    let listener = TcpListener::bind(address).await?;

    println!("\n[Teralow LAN Host]");
    println!("Listening on {}", listener.local_addr()?);
    println!("Waiting for connection...");
    println!("No remote desktop access is enabled.");

    loop {
        let (mut stream, peer) = listener.accept().await?;

        println!("\nIncoming connection from {peer}");

        // For now, process a single greeting at a time
        // Secure multi-client handling can come later
        match handle_connection(&mut stream).await {
            Ok(()) => println!("Protocol greeting completed: {peer}"),
            Err(error) => eprintln!("Connection rejected: {error}"),
        }

        // Explicitly close the test connection
        let _ = stream.shutdown().await;
    }
}

async fn handle_connection(stream: &mut TcpStream) -> NetworkResult<()> {
    let mut greeting = [0u8; 8];

    // A peer mus send the complete greeting within 5 seconds
    timeout(Duration::from_secs(5), stream.read_exact(&mut greeting)).await??;

    if &greeting != CLIENT_HELLO {
        return Err("Unsupported Teralow greeting".into());
    }

    stream.write_all(HOST_HELLO).await?;

    Ok(())
}

pub async fn connect_to_host(address: &str) -> NetworkResult<()> {
    println!("\n[Teralow LAN client]");
    println!("Connecting to {address}...");

    let target: SocketAddr = address.parse()?;

    let mut stream = timeout(Duration::from_secs(5), TcpStream::connect(target)).await??;

    println!("TCP connection established.");

    stream.write_all(CLIENT_HELLO).await?;

    let mut response = [0u8; 8];

    timeout(Duration::from_secs(5), stream.read_exact(&mut response)).await??;

    if &response != HOST_HELLO {
        return Err("Unexpected host response".into());
    }

    println!("Teralow protocol greeting successful!");
    println!("Pairing is not implemented yet.");
    println!("No video or remote input has been authorised.");

    stream.shutdown().await?;

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[tokio::test]
    async fn valid_client_greeting_accepted() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

        let address = listener.local_addr().unwrap();

        let host = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            handle_connection(&mut stream).await.unwrap();
        });

        let mut client = TcpStream::connect(address).await.unwrap();

        client.write_all(CLIENT_HELLO).await.unwrap();

        let mut response = [0u8; 8];
        client.read_exact(&mut response).await.unwrap();

        assert_eq!(&response, HOST_HELLO);

        host.await.unwrap();
    }

    #[tokio::test]
    async fn invalid_client_greeting_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

        let address = listener.local_addr().unwrap();

        let host = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            assert!(handle_connection(&mut stream).await.is_err());
        });

        let mut client = TcpStream::connect(address).await.unwrap();

        client.write_all(b"INVALID!").await.unwrap();
        client.shutdown().await.unwrap();

        host.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn silent_client_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();

        let address = listener.local_addr().unwrap();

        let host = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            handle_connection(&mut stream).await
        });

        let _client = TcpStream::connect(address).await.unwrap();

        let result = host.await.unwrap();

        assert!(
            result.is_err(),
            "An unresponsive client should be rejected."
        );
    }
}
