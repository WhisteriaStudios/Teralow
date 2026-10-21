use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
};
use ashpd::enumflags2::BitFlags;

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;

use std::error::Error;
use std::os::fd::AsRawFd;

type CaptureResult<T> = Result<T, Box<dyn Error>>;

// Read a frame through GStreamer and save it as a PNG.
fn save_first_frame(pipewire_fd: i32, node_id: u32) -> CaptureResult<()> {
    gst::init()?;

    println!("\n[First Frame]");
    println!("Connecting GStreamer to PipeWire node {node_id}...");

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

    println!("Waiting for a desktop frame...");

    let result = (|| -> CaptureResult<()> {
        let sample = sink
            .try_pull_sample(gst::ClockTime::from_seconds(5))
            .ok_or("No desktop frame received within 5 seconds")?;

        let caps = sample
            .caps()
            .ok_or("Captured sample has no format information")?;

        let info = gst_video::VideoInfo::from_caps(caps)?;

        if info.format() != gst_video::VideoFormat::Rgb {
            return Err("Expected RGB video format".into());
        }

        let width = info.width();
        let height = info.height();

        println!("Received frame: {width}x{height}");

        let buffer = sample
            .buffer()
            .ok_or("Captured sample contains no buffer")?;

        let map = buffer.map_readable()?;
        let data = map.as_slice();

        // Copy each row separately to account for padding.
        let row_bytes = width as usize * 3;
        let stride = usize::try_from(info.stride()[0])?;
        let offset = info.offset()[0];

        if stride < row_bytes {
            return Err("Invalid video row stride".into());
        }

        let mut pixels = vec![0u8; row_bytes * height as usize];

        for y in 0..height as usize {
            let source_start = offset + y * stride;
            let source_end = source_start + row_bytes;

            let source_row = data
                .get(source_start..source_end)
                .ok_or("Video buffer is smaller than expected")?;

            let dest_start = y * row_bytes;
            let dest_end = dest_start + row_bytes;

            pixels[dest_start..dest_end].copy_from_slice(source_row);
        }

        let filename = "teralow-first-frame.png";

        image::save_buffer(filename, &pixels, width, height, image::ColorType::Rgb8)?;

        println!("Screenshot saved: {filename}");

        Ok(())
    })();

    // Always stop the pipeline, even if capturing failed.
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
        match save_first_frame(remote_fd.as_raw_fd(), node_id) {
            Ok(()) => println!("First Frame: SUCCESS"),
            Err(error) => eprintln!("First Frame failed: {error}"),
        }
    }

    session.close().await?;

    println!("\nScreen capture stopped. Session closed.");

    Ok(())
}
