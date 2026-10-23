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

fn capture_encoded_video(pipewire_fd: i32, node_id: u32) -> CaptureResult<()> {
    gst::init()?;

    println!("\n[NVENC Encoded Video");
    println!("Connecting to PipeWire node {node_id}");

    let description = format!(
        "pipewiresrc fd={pipewire_fd} path={node_id} \
        do-timestamp=true ! \
        queue max-size-buffers=2 leaky=downstream ! \
        videoconvert ! \
        videorate ! \
        video/x-raw,format=NV12,framerate=60/1 ! \
        nvh264enc preset=p1 tune=ultra-low-latency \
        zerolatency=true bframes=0 bitrate=8000 ! \
        h264parse config-interval=-1 ! \
        video/x-h264,stream-format=byte-stream,alignment=au ! \
        appsink name=encoded_sink sync=false max-buffers=2 drop=true"
    );

    let element = gst::parse::launch(&description)?;

    let pipeline = element
        .downcast::<gst::Pipeline>()
        .map_err(|_| "Failed to create GStreamer Pipeline")?;

    let sink = pipeline
        .by_name("encoded_sink")
        .ok_or("Could not find encoded_sink")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| "encoded_sink is not an AppSink")?;

    let bus = pipeline
        .bus()
        .ok_or("GStreamer pipeline has no message bus")?;

    pipeline.set_state(gst::State::Playing)?;

    let total_sec = 20;

    let result = (|| -> CaptureResult<()> {
        let duration = Duration::from_secs(total_sec);
        let start = Instant::now();
        let mut last_repot = Instant::now();

        let mut buffer_count: u64 = 0;
        let mut total_bytes: u64 = 0;
        let mut keyframe_candidates: u64 = 0;

        println!("Receiving H.264 video buffers for {total_sec} seconds");

        while start.elapsed() < duration {
            // Look for encoder or pipeline error
            if let Some(message) = bus.timed_pop_filtered(
                gst::ClockTime::ZERO,
                &[gst::MessageType::Error, gst::MessageType::Eos],
            ) {
                match message.view() {
                    gst::MessageView::Error(error) => {
                        return Err(format!(
                            "GStreamer error: {} ({:?})",
                            error.error(),
                            error.debug()
                        )
                        .into());
                    }

                    gst::MessageView::Eos(..) => {
                        return Err("Video stream ended unexpectedly".into());
                    }

                    _ => {}
                }
            }

            // Request the next compressed video sample
            let sample = sink.try_pull_sample(gst::ClockTime::from_mseconds(100));

            if let Some(sample) = sample {
                if let Some(buffer) = sample.buffer() {
                    // Map the encoded bytes into readable memory
                    let map = buffer.map_readable()?;
                    let encoded_bytes = map.as_slice();

                    buffer_count += 1;
                    total_bytes += encoded_bytes.len() as u64;

                    if !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT) {
                        keyframe_candidates += 1;
                    }
                }
            }

            // Report stats once a second
            if last_repot.elapsed() >= Duration::from_secs(1) {
                let elapsed = start.elapsed().as_secs_f64();

                let average_rate = buffer_count as f64 / elapsed;

                let bitrate_mbps = total_bytes as f64 * 8.0 / elapsed / 1_000_000.0;

                println!(
                    "Elapsed: {:.1}s | Buffers: {} | Rate: {:.1}/s | Bitrate: {:.2} Mbps",
                    elapsed, buffer_count, average_rate, bitrate_mbps
                );

                last_repot = Instant::now();
            }
        }

        let elapsed = start.elapsed().as_secs_f64();

        if buffer_count == 0 {
            return Err("No encoded H.264 buffers received".into());
        }

        println!("\n[Encoded Video Results]");
        println!("Duration: {:.2}s", elapsed);
        println!("Buffers received: {buffer_count}");
        println!("Total encoded bytes: {total_bytes}");
        println!("Keyframe candidates: {keyframe_candidates}");

        println!(
            "Average bitrate: {:.2} Mbps",
            total_bytes as f64 * 8.0 / elapsed / 1_000_000.0
        );

        Ok(())
    })();

    // Release Gstreamer resources, even if capture fails
    let shutdown_result = pipeline.set_state(gst::State::Null);

    result?;
    shutdown_result?;

    Ok(())
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
        match capture_encoded_video(remote_fd.as_raw_fd(), node_id) {
            Ok(()) => println!("Encoded Capture: SUCCESS"),
            Err(error) => eprintln!("Encoded Capture failed: {error}"),
        }
    }

    session.close().await?;

    println!("\nScreen capture stopped. Session closed.");

    Ok(())
}
