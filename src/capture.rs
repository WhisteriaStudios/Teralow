use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use ashpd::enumflags2::BitFlags;

use gstreamer as gst;
use gstreamer::prelude::*;

use std::error::Error;
use std::os::fd::AsRawFd;
use std::time::Duration;

type CaptureResult<T> = Result<T, Box<dyn Error>>;

fn record_desktop(pipewire_fd: i32, node_id: u32) -> CaptureResult<()> {
    gst::init()?;

    println!("\n[NVIDIA NVENC Recording]");
    println!("Connecting to PipeWire node {node_id}...");

    let filename = "teralow-nvenc-test.mvk";

    let description = format!(
        "pipewiresrc fd={pipewire_fd} path={node_id} \
        do-timestamp=true ! \
        queue max-size-buffers=2 leaky=downstream ! \
        videoconvert ! \
        videorate ! \
        video/x-raw,format=NV12,framerate=60/1 ! \
        nvh264enc preset=p1 tune=ultra-low-latency \
        zerolatency=true bframes=0 bitrate=8000 ! \
        h264parse ! \
        matroskamux ! \
        filesink location={filename}"
    );

    let element = gst::parse::launch(&description)?;

    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "Failed to create GStreamer pipeline")?;

    let bus = pipeline
        .bus()
        .ok_or("GStreamer pipeline has no message bus");

    println!("Encoder: NVIDIA H.264");
    println!("Target: 1080p / 60 FPS / 8 Mbps");
    println!("Output: {filename}");

    pipeline.set_state(gst::State::Playing)?;

    let vid_secs = 20;
    let result = (|| -> CaptureResult<()> {
        println!("\nRecording desktop for {} seconds...", vid_secs);
        println!("Move some windows around!");

        std::thread::sleep(Duration::from_secs(vid_secs));

        println!("Finishing recording...");

        // Ask GStreamer to finish writing the video
        if !pipeline.send_event(gst::event::Eos::new()) {
            return Err("Could not send end-of-stream event".into());
        }

        // Wait for the file to finish being written,
        // or for an encoder/pipeline error
        let message = bus?.timed_pop_filtered(
            gst::ClockTime::from_seconds(vid_secs),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        );

        match message {
            Some(message) => match message.view() {
                gst::MessageView::Eos(..) => {
                    println!("Recording finalised successfully.");
                    Ok(())
                }

                gst::MessageView::Error(error) => {
                    Err(format!("GStreamer error: {} ({:?})", error.error(), error.debug()).into())
                }

                _ => Err("Unexpected GStreamer message".into()),
            },

            None => Err("Timed out waiting for recording to finish".into()),
        }
    })();

    // Shut down even if the recording failed
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
        match record_desktop(remote_fd.as_raw_fd(), node_id) {
            Ok(()) => println!("NVENC Recording: SUCCESS"),
            Err(error) => eprint!("NVENC Recording: {error}"),
        }
    }

    session.close().await?;

    println!("\nScreen capture stopped. Session closed.");

    Ok(())
}
