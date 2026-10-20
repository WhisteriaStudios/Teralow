use gstreamer as gst; // For GStreamer bindings

pub fn scan_encoders() -> Result<(), gst::glib::Error> {
    gst::init()?;

    println!("\n[Video Encoders]");

    let encoders = [
        ("nvh264enc", "NVIDIA NVENC H.264"),
        ("nvh265enc", "NVIDIA NVENC H.265 / HEVC"),
        ("nvav1enc", "NVIDIA NVENC AV1"),
        ("vah264enc", "VA API H.264"),
        ("vah265enc", "VA API H.265 / HEVC"),
        ("x264enc", "x264 H.264"),
        ("x265enc", "x265 H.265"),
    ];

    for (element, name) in encoders {
        let available = gst::ElementFactory::find(element).is_some();

        if available {
            println!("[FOUND] {name}");
        } else {
            println!("[MISSING] {name}");
        }
    }

    Ok(())
}
