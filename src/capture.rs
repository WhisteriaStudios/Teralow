use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use ashpd::enumflags2::BitFlags;

pub async fn start_capture() -> ashpd::Result<()> {
    println!("\n[Wayland Capture]");
    println!("Connecting to KDE Desktop Portal...");

    // Connect to the Screencast portal
    let portal = Screencast::new().await?;

    // Create a new screen sharing session
    let session = portal.create_session(Default::default()).await?;

    // Configure the capture request
    portal
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Metadata)
                .set_sources(BitFlags::from_flag(SourceType::Monitor))
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await?;

    println!("Requesting monitor access...");

    // KDE should show it's screen sharing dialogue
    let response = portal
        .start(&session, None, Default::default())
        .await?
        .response()?;

    println!("Screen sharing permission granted!");

    // Open the authorised PipeWire connection
    let remote_fd = portal
        .open_pipe_wire_remote(&session, Default::default())
        .await?;

    println!("PipeWire remote opened successfully!");
    println!("File descriptor: {:?}", remote_fd);

    // Display the available PipeWire streams
    for stream in response.streams() {
        println!("\n[PipeWire Stream]");
        println!("Node ID: {}", stream.pipe_wire_node_id());
        println!("Size: {:?}", stream.size());
        println!("Position: {:?}", stream.position());
    }

    println!("\nCapture session initialised.");

    // Start the capture process
    println!("\nScreen capture started. Press ENTER to stop.");

    tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .expect("Failed to read input");
    })
    .await
    .expect("Input task failed");

    session.close().await?;

    println!("\nScreen capture stopped. Session closed.");

    Ok(())
}
