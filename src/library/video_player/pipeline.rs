use std::sync::Arc;

use gstreamer as gst;
use gstreamer_play as gst_play;
use gtk::prelude::*;
use parking_lot::Mutex;

pub(super) fn build_play(
    volume: f64,
    muted: bool,
    api_key: Arc<Mutex<Option<String>>>,
) -> Result<(gst_play::Play, gtk::gdk::Paintable), String> {
    let (sink, paintable) = video_sink()?;
    let play = gst_play::Play::new(None::<gst_play::PlayVideoRenderer>);
    let mut config = play.config();
    config.set_position_update_interval(250);
    config.set_seek_accurate(true);
    play.set_config(config).map_err(|e| e.to_string())?;
    let pipeline = play.pipeline();
    pipeline.set_property("video-sink", &sink);
    if let Ok(tempo) = gst::ElementFactory::make("scaletempo").build() {
        pipeline.set_property("audio-filter", &tempo);
    }
    connect_api_key_header(&pipeline, api_key);
    play.set_volume(volume);
    play.set_mute(muted);
    Ok((play, paintable))
}

pub(super) fn frame_duration(numer: i32, denom: i32) -> Option<gst::ClockTime> {
    if numer <= 0 || denom <= 0 {
        return None;
    }
    Some(gst::ClockTime::from_nseconds(
        1_000_000_000 * denom as u64 / numer as u64,
    ))
}

fn video_sink() -> Result<(gst::Element, gtk::gdk::Paintable), String> {
    let gtk_sink = gst::ElementFactory::make("gtk4paintablesink")
        .build()
        .map_err(|_| "The GStreamer GTK 4 video sink (gst-plugin-gtk4) is not installed.")?;
    let paintable = gtk_sink.property::<gtk::gdk::Paintable>("paintable");
    if !paintable.has_property("gl-context")
        || paintable
            .property::<Option<gtk::gdk::GLContext>>("gl-context")
            .is_none()
    {
        return Ok((gtk_sink, paintable));
    }
    let gl_sink = gst::ElementFactory::make("glsinkbin")
        .property("sink", &gtk_sink)
        .build()
        .map_err(|e| e.to_string())?;
    Ok((gl_sink, paintable))
}

fn connect_api_key_header(pipeline: &gst::Element, api_key: Arc<Mutex<Option<String>>>) {
    pipeline.connect("source-setup", false, move |args| {
        let source = args[1].get::<gst::Element>().ok()?;
        let key = api_key.lock().clone()?;
        if source.has_property("extra-headers") {
            let headers = gst::Structure::builder("extra-headers")
                .field("x-api-key", key)
                .build();
            source.set_property("extra-headers", headers);
        }
        None
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_duration_handles_ntsc_and_unknown_rates() {
        assert_eq!(
            frame_duration(30, 1),
            Some(gst::ClockTime::from_nseconds(33_333_333))
        );
        assert_eq!(
            frame_duration(30000, 1001),
            Some(gst::ClockTime::from_nseconds(33_366_666))
        );
        assert_eq!(frame_duration(0, 1), None);
    }
}
