use super::ProfileContext;
use super::ProfileType;
use super::StreamType;
use super::TranscodingProfile;

use crate::NightfallError;

#[derive(Debug)]
pub struct H264TransmuxProfile;

impl TranscodingProfile for H264TransmuxProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transmux
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Video
    }

    fn name(&self) -> &str {
        "H264TransmuxProfile"
    }

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let start_num = ctx.output_ctx.start_num.to_string();
        let stream = format!("0:{}", ctx.input_ctx.stream);
        let init_seg = format!("{}_init.mp4", &start_num);
        let seg_name = format!("{}/%d.m4s", ctx.output_ctx.outdir);
        let outdir = format!("{}/playlist.m3u8", ctx.output_ctx.outdir);

        let mut args = vec![
            "-y".into(),
            "-ss".into(),
            (ctx.output_ctx.start_num * ctx.output_ctx.target_gop).to_string(),
            "-i".into(),
            ctx.file.clone(),
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "copy".into(),
        ];

        args.append(&mut vec![
            "-start_at_zero".into(),
            "-vsync".into(),
            "passthrough".into(),
            "-avoid_negative_ts".into(),
            "disabled".into(),
            "-max_muxing_queue_size".into(),
            "2048".into(),
        ]);

        args.append(&mut vec![
            "-f".into(),
            "hls".into(),
            "-start_number".into(),
            start_num,
        ]);

        // needed so that in progress segments are named `tmp` and then renamed after the data is
        // on disk.
        // This in theory practically prevents the web server from returning a segment that is
        // in progress.
        args.append(&mut vec![
            "-hls_flags".into(),
            "temp_file".into(),
            "-max_delay".into(),
            "5000000".into(),
        ]);

        // args needed so we can distinguish between init fragments for new streams.
        // Basically on the web seeking works by reloading the entire video because of
        // discontinuity issues that browsers seem to not ignore like mpv.
        args.append(&mut vec!["-hls_fmp4_init_filename".into(), init_seg]);

        args.append(&mut vec![
            "-hls_time".into(),
            ctx.output_ctx.target_gop.to_string(),
        ]);

        args.append(&mut get_discont_flags(&ctx));

        args.append(&mut vec![
            "-force_key_frames".into(),
            format!("expr:gte(t,n_forced*{})", ctx.output_ctx.target_gop),
        ]);

        args.append(&mut vec!["-hls_segment_type".into(), 1.to_string()]);
        args.append(&mut vec![
            "-loglevel".into(),
            "info".into(),
            "-progress".into(),
            "pipe:1".into(),
        ]);
        args.append(&mut vec!["-hls_segment_filename".into(), seg_name]);
        args.push(outdir);

        Some(args)
    }

    /// This profile technically could work on any codec since the codec is just `copy` here, but
    /// the container doesnt support it, so we will be constricting it down.
    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ctx.output_ctx.height.is_some()
            || ctx.output_ctx.width.is_some()
            || ctx.output_ctx.bitrate.is_some()
        {
            return Err(NightfallError::ProfileNotSupported(
                "Transmuxed streams cannot be resized.".into(),
            ));
        }

        if ctx.input_ctx.codec == ctx.output_ctx.codec
            && matches!(ctx.input_ctx.codec.as_str(), "h264" | "av1")
        {
            return Ok(());
        }

        Err(NightfallError::ProfileNotSupported(
            "Profile only supports matching h264 or av1 input and output codecs.".into(),
        ))
    }

    fn tag(&self) -> &str {
        "h264_copy"
    }
}

#[derive(Debug)]
pub struct H264TranscodeProfile;

impl TranscodingProfile for H264TranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transcode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Video
    }

    fn name(&self) -> &str {
        "H264TranscodeProfile"
    }

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let start_num = ctx.output_ctx.start_num.to_string();
        let stream = format!("0:{}", ctx.input_ctx.stream);
        let init_seg = format!("{}_init.mp4", &start_num);
        let seg_name = format!("{}/%d.m4s", ctx.output_ctx.outdir);
        let outdir = format!("{}/playlist.m3u8", ctx.output_ctx.outdir);

        let mut args = vec![
            "-y".into(),
            "-ss".into(),
            (ctx.output_ctx.start_num * ctx.output_ctx.target_gop).to_string(),
            "-i".into(),
            ctx.file.clone(),
            "-copyts".into(),
            "-map".into(),
            stream,
            "-c:0".into(),
            "libx264".into(),
            "-preset".into(),
            "veryfast".into(),
        ];

        if let Some(filter) = browser_h264_filter(&ctx) {
            args.push("-vf".into());
            args.push(filter);
        }

        if let Some(profile) = ctx.output_ctx.video_profile.as_ref() {
            args.push("-profile:v".into());
            args.push(profile.clone());
        }
        if let Some(level) = ctx.output_ctx.video_level {
            args.push("-level:v".into());
            args.push(format!("{}.{}", level / 10, level % 10));
        }
        if let Some(pixel_format) = ctx.output_ctx.pixel_format.as_ref() {
            args.push("-pix_fmt".into());
            args.push(pixel_format.clone());
        }
        for (flag, value) in [
            ("-color_range", ctx.output_ctx.color_range.as_ref()),
            ("-colorspace", ctx.output_ctx.color_space.as_ref()),
            ("-color_trc", ctx.output_ctx.color_transfer.as_ref()),
            ("-color_primaries", ctx.output_ctx.color_primaries.as_ref()),
        ] {
            if let Some(value) = value {
                args.push(flag.into());
                args.push(value.clone());
            }
        }

        if let Some(bitrate) = ctx.output_ctx.bitrate {
            args.push("-b:v".into());
            args.push(bitrate.to_string());
        }

        args.append(&mut vec![
            "-vsync".into(),
            "passthrough".into(),
            "-avoid_negative_ts".into(),
            "make_non_negative".into(),
            "-max_muxing_queue_size".into(),
            "2048".into(),
        ]);

        args.append(&mut vec![
            "-f".into(),
            "hls".into(),
            "-start_number".into(),
            start_num,
        ]);

        args.append(&mut get_discont_flags(&ctx));

        // needed so that in progress segments are named `tmp` and then renamed after the data is
        // on disk.
        // This in theory practically prevents the web server from returning a segment that is
        // in progress.
        args.append(&mut vec![
            "-hls_flags".into(),
            "temp_file".into(),
            "-max_delay".into(),
            "5000000".into(),
        ]);

        // args needed so we can distinguish between init fragments for new streams.
        // Basically on the web seeking works by reloading the entire video because of
        // discontinuity issues that browsers seem to not ignore like mpv.
        args.append(&mut vec!["-hls_fmp4_init_filename".into(), init_seg]);
        args.append(&mut vec![
            "-hls_time".into(),
            ctx.output_ctx.target_gop.to_string(),
        ]);
        args.append(&mut vec![
            "-force_key_frames".into(),
            format!("expr:gte(t,n_forced*{})", ctx.output_ctx.target_gop),
        ]);

        args.append(&mut vec!["-hls_segment_type".into(), 1.to_string()]);
        args.append(&mut vec![
            "-loglevel".into(),
            "info".into(),
            "-progress".into(),
            "pipe:1".into(),
        ]);
        args.append(&mut vec!["-hls_segment_filename".into(), seg_name]);
        args.push(outdir);

        Some(args)
    }

    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ctx.output_ctx.codec == "h264" {
            return Ok(());
        }

        Err(NightfallError::ProfileNotSupported(format!(
            "Got output codec {} but profile only supports `h264`.",
            ctx.output_ctx.codec
        )))
    }

    fn tag(&self) -> &str {
        "h264"
    }
}

fn browser_h264_filter(ctx: &ProfileContext) -> Option<String> {
    let scale = ctx.output_ctx.height.map(|height| {
        let width = ctx.output_ctx.width.unwrap_or(-2);
        format!("scale={width}:{height}")
    });
    let format = ctx
        .output_ctx
        .pixel_format
        .as_deref()
        .unwrap_or("yuv420p");

    match ctx.output_ctx.hdr_transfer.as_deref() {
        Some(transfer) => hdr_to_sdr_filter(scale.as_deref(), format, transfer, ctx.output_ctx.hdr_peak_nits),
        None => Some(match scale {
            Some(scale) => format!("{scale},format={format}"),
            None => format!("format={format}"),
        }),
    }
}

fn hdr_to_sdr_filter(
    scale: Option<&str>,
    pixel_format: &str,
    transfer: &str,
    peak_nits: Option<f64>,
) -> Option<String> {
    let sample = "min(max(val,0),1)";
    let linearize = match transfer {
        "smpte2084" => format!(
            "pow(max(pow({sample},0.0126833135)-0.8359375,0)/(18.8515625-18.6875*pow({sample},0.0126833135)),6.277394636)*100"
        ),
        "arib-std-b67" => format!(
            "if(lte({sample},0.5),pow({sample},2)/3,(exp(({sample}-0.55991073)/0.17883277)+0.28466892)/12)*10"
        ),
        _ => return None,
    };
    let bt709_oetf =
        "if(lt(max(val,0),0.018),4.5*max(val,0),1.099*pow(max(val,0),0.45)-0.099)";
    let peak = peak_nits.unwrap_or(1_000.0).clamp(100.0, 10_000.0) / 100.0;
    let mut filters = Vec::new();
    if let Some(scale) = scale {
        filters.push(scale.to_string());
    }
    filters.extend([
        "format=gbrpf32le".into(),
        format!("lutrgb=r='{linearize}':g='{linearize}':b='{linearize}'"),
        "setparams=range=pc:color_primaries=bt2020:color_trc=linear:colorspace=gbr".into(),
        "colorchannelmixer=rr=1.660491:rg=-0.587641:rb=-0.072850:gr=-0.124550:gg=1.132900:gb=-0.008349:br=-0.018151:bg=-0.100579:bb=1.118730".into(),
        "setparams=range=pc:color_primaries=bt709:color_trc=linear:colorspace=gbr".into(),
        format!("tonemap=hable:desat=0:peak={peak:.4}"),
        format!("lutrgb=r='{bt709_oetf}':g='{bt709_oetf}':b='{bt709_oetf}'"),
        "setparams=range=pc:color_primaries=bt709:color_trc=bt709:colorspace=gbr".into(),
        format!("colorspace=ispace=gbr:iprimaries=bt709:itrc=bt709:irange=pc:space=bt709:primaries=bt709:trc=bt709:range=tv:format={pixel_format}"),
        "sidedata=mode=delete:type=MASTERING_DISPLAY_METADATA".into(),
        "sidedata=mode=delete:type=CONTENT_LIGHT_LEVEL".into(),
        "sidedata=mode=delete:type=DYNAMIC_HDR_PLUS".into(),
        "sidedata=mode=delete:type=DOVI_RPU_BUFFER".into(),
        "sidedata=mode=delete:type=DOVI_METADATA".into(),
        "sidedata=mode=delete:type=DYNAMIC_HDR_VIVID".into(),
        "sidedata=mode=delete:type=AMBIENT_VIEWING_ENVIRONMENT".into(),
    ]);
    Some(filters.join(","))
}

pub(crate) fn hardware_h264_contract_supported(
    ctx: &ProfileContext,
) -> Result<(), NightfallError> {
    if ctx.output_ctx.codec != "h264" {
        return Err(NightfallError::ProfileNotSupported(
            "Hardware browser profile only supports h264 output.".into(),
        ));
    }
    if ctx.output_ctx.hdr_transfer.is_some() {
        return Err(NightfallError::ProfileNotSupported(
            "Hardware profile has no verified HDR-to-SDR filter; use the software fallback."
                .into(),
        ));
    }
    if !matches!(ctx.input_ctx.pix_fmt.as_str(), "yuv420p" | "nv12") {
        return Err(NightfallError::ProfileNotSupported(format!(
            "Hardware profile has no verified 8-bit 4:2:0 conversion for {}.",
            ctx.input_ctx.pix_fmt
        )));
    }
    Ok(())
}

pub(crate) fn append_h264_output_signalling(
    args: &mut Vec<String>,
    ctx: &ProfileContext,
) {
    if let Some(profile) = ctx.output_ctx.video_profile.as_ref() {
        args.extend(["-profile:v".into(), profile.clone()]);
    }
    if let Some(level) = ctx.output_ctx.video_level {
        args.extend([
            "-level:v".into(),
            format!("{}.{}", level / 10, level % 10),
        ]);
    }
    for (flag, value) in [
        ("-color_range", ctx.output_ctx.color_range.as_ref()),
        ("-colorspace", ctx.output_ctx.color_space.as_ref()),
        ("-color_trc", ctx.output_ctx.color_transfer.as_ref()),
        ("-color_primaries", ctx.output_ctx.color_primaries.as_ref()),
    ] {
        if let Some(value) = value {
            args.extend([flag.into(), value.clone()]);
        }
    }
}

#[derive(Debug)]
pub struct RawVideoTranscodeProfile;

impl TranscodingProfile for RawVideoTranscodeProfile {
    fn profile_type(&self) -> ProfileType {
        ProfileType::Transcode
    }

    fn stream_type(&self) -> StreamType {
        StreamType::Video
    }

    fn name(&self) -> &str {
        "RawVideoTranscodeProfile"
    }

    fn build(&self, ctx: ProfileContext) -> Option<Vec<String>> {
        let mut args = vec!["-y".into()];

        if let Some(seek) = ctx.input_ctx.seek {
            let flag = if seek.is_positive() {
                "-ss".into()
            } else {
                "-sseof".into()
            };

            args.push(flag);
            args.push(seek.to_string());
        }

        if let Some(max_to_transcode) = ctx.output_ctx.max_to_transcode {
            args.push("-t".into());
            args.push(max_to_transcode.to_string());
        }

        args.append(&mut vec![
            "-map".into(),
            format!("0:{}", ctx.input_ctx.stream),
        ]);
        args.append(&mut vec!["-c:v".into(), "rawvideo".into()]);
        args.append(&mut vec![
            "-flags2".into(),
            "-pix_fmt".into(),
            "rgb24".into(),
        ]);
        args.append(&mut vec!["-preset".into(), "ultrafast".into()]);

        if let Some(height) = ctx.output_ctx.height {
            let width = ctx.output_ctx.width.unwrap_or(-2);

            args.append(&mut vec![
                "-vf".into(),
                format!("scale={}:{}", height, width),
            ]);
        }

        args.append(&mut vec!["-f".into(), "data".into(), "-".into()]);

        Some(args)
    }

    fn supports(&self, ctx: &ProfileContext) -> Result<(), NightfallError> {
        if ctx.output_ctx.codec == "rawvideo" {
            return Ok(());
        }

        Err(NightfallError::ProfileNotSupported(format!(
            "Codec {} is not supported.",
            ctx.output_ctx.codec
        )))
    }

    fn tag(&self) -> &str {
        "rawvideo"
    }

    fn is_stdio_stream(&self) -> bool {
        true
    }
}

pub(super) fn get_discont_flags(ctx: &ProfileContext) -> Vec<String> {
    // these args are needed if we start a new stream in the middle of a old one, such as when
    // seeking. These args will reset the base decode ts to equal the earliest presentation
    // timestamp.
    if ctx.output_ctx.start_num > 0 {
        vec![
            "-hls_segment_options".into(),
            "movflags=frag_custom+dash+delay_moov+frag_discont".into(),
        ]
    } else {
        vec![
            "-hls_segment_options".into(),
            "movflags=frag_custom+dash+delay_moov".into(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser_h264_context(hdr: bool) -> ProfileContext {
        ProfileContext {
            file: "source.mkv".into(),
            input_ctx: super::super::InputCtx {
                stream: 0,
                codec: "av1".into(),
                pix_fmt: "yuv420p10le".into(),
                profile: "Main".into(),
                ..Default::default()
            },
            output_ctx: super::super::OutputCtx {
                codec: "h264".into(),
                bitrate: Some(10_000_000),
                width: Some(1920),
                height: Some(1080),
                video_profile: Some("high".into()),
                video_level: Some(40),
                pixel_format: Some("yuv420p".into()),
                color_range: Some("tv".into()),
                color_space: Some("bt709".into()),
                color_transfer: Some("bt709".into()),
                color_primaries: Some("bt709".into()),
                hdr_transfer: hdr.then(|| "smpte2084".into()),
                hdr_peak_nits: hdr.then_some(1_000.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    }

    #[test]
    fn h264_fallback_has_a_complete_browser_output_contract() {
        let args = H264TranscodeProfile
            .build(browser_h264_context(false))
            .unwrap();
        assert_eq!(value_after(&args, "-c:0"), Some("libx264"));
        assert_eq!(value_after(&args, "-profile:v"), Some("high"));
        assert_eq!(value_after(&args, "-level:v"), Some("4.0"));
        assert_eq!(value_after(&args, "-pix_fmt"), Some("yuv420p"));
        assert_eq!(value_after(&args, "-colorspace"), Some("bt709"));
        assert_eq!(value_after(&args, "-color_trc"), Some("bt709"));
        assert_eq!(value_after(&args, "-color_primaries"), Some("bt709"));
        assert_eq!(value_after(&args, "-color_range"), Some("tv"));
    }

    #[test]
    fn hdr_fallback_tone_maps_and_removes_hdr_side_data() {
        let args = H264TranscodeProfile
            .build(browser_h264_context(true))
            .unwrap();
        let filter = value_after(&args, "-vf").unwrap();
        assert!(filter.contains("color_trc=linear"));
        assert!(filter.contains("tonemap=hable"));
        assert!(filter.contains("primaries=bt709:trc=bt709"));
        assert!(filter.contains("type=MASTERING_DISPLAY_METADATA"));
        assert_eq!(value_after(&args, "-pix_fmt"), Some("yuv420p"));
    }

    #[test]
    fn unverified_hardware_paths_defer_ten_bit_and_hdr_sources_to_software() {
        let context = browser_h264_context(true);
        assert!(hardware_h264_contract_supported(&context).is_err());
        let mut context = browser_h264_context(false);
        assert!(hardware_h264_contract_supported(&context).is_err());
        context.input_ctx.pix_fmt = "yuv420p".into();
        assert!(hardware_h264_contract_supported(&context).is_ok());
    }
}
