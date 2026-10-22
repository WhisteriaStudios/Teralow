use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use ashpd::enumflags2::BitFlags;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;

use std::error::Error;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

type CaptureResult<T> = Result<T, Box<dyn Error>>;

fn measure_capture(pipewire_fd: i32, node_id: u32) -> CaptureResult<()> {
    gst::init()?;

    println!("\n[Continuous Capture]");
    println!("Connecting to PipeWire node {node_id}...");

    let description = format!(
        "pipewiresrc fd={pipewire_fd} path={node_id} \
        do-timestamp=true ! \
        videoconvert ! \
        video/x-raw,format=RGB ! \
        appsink name=frame_sink sync=false max-buffers=1 drop=true"
    );

    let element = gst::parse::launch(&description)?;

    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "Failed to create GStreamer pipeline")?;

    let sink = pipeline
        .by_name("frame_sink")
        .ok_or("Could not find frame_sink")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| "frame_sink is not an AppSink")?;

    pipeline.set_state(gst::State::Playing)?;

    let result = (|| -> CaptureResult<()> {
        let duration = Duration::from_secs(10);
        let start = Instant::now();

        let mut frame_count: u64 = 0;
        let mut last_report = Instant::now();

        println!("Capturing for 10 seconds...");
        println!("Move some windows around to generate activity.");

        while start.elapsed() < duration {
            let sample = sink.try_pull_sample(gst::ClockTime::from_mseconds(100));

            if let Some(sample) = sample {
                if sample.buffer().is_some() {
                    frame_count += 1;
                }
            }

            // Print a status update approx once per second
            if last_report.elapsed() >= Duration::from_secs(1) {
                let seconds = start.elapsed().as_secs_f64();

                let average_fps = frame_count as f64 / seconds;

                println!(
                    "Elapsed: {:.1}s | Frames: {} | Average: {:.1} FPS",
                    seconds, frame_count, average_fps
                );

                last_report = Instant::now();
            }
        }

        let elapsed = start.elapsed().as_secs_f64();
        let average_fps = frame_count as f64 / elapsed;

        println!("\n[Capture Results]");
        println!("Duration: {:.2} seconds", elapsed);
        println!("Frames received: {frame_count}");
        println!("Average delivery rate: {:.2} FPS", average_fps);

        if frame_count == 0 {
            return Err("No frames were received".into());
        }

        Ok(())
    })();

    // Stop GStreamer whether the experiment passed or failed
    pipeline.set_state(gst::State::Null)?;

    result
}

pub async fn start_capture() -> ashpd::Result<()> {
    println!("\n[Wayland Capture]");
    println!("Connecting to KDE Desktop Portal...");

    let portal = Screencast::new().await?;
    let session = portal.create_session(Default::default()).await?;

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

    let response = portal
        .start(&session, None, Default::default())
        .await?
        .response()?;

    println!("Screen sharing permission granted!");

    let remote_fd = portal
        .open_pipe_wire_remote(&session, Default::default())
        .await?;

    println!("PipeWire remote opened successfully!");

    for stream in response.streams() {
        let node_id = stream.pipe_wire_node_id();

        println!("\n[PipeWire Stream]");
        println!("Node ID: {node_id}");
        println!("Size: {:?}", stream.size());
        println!("Position: {:?}", stream.position());

        // The portal-authorised descriptor remains alive
        // while GStreamer consumes the selected stream.
        match measure_capture(remote_fd.as_raw_fd(), node_id) {
            Ok(()) => println!("Continuous Capture: SUCCESS"),
            Err(error) => eprintln!("Continuous Capture failed: {error}"),
        }
    }

    session.close().await?;

    println!("\nScreen capture stopped. Session closed.");

    Ok(())
}
