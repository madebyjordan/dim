use super::ffprobe::Stream;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VideoCapabilityRequest {
    pub content_type: String,
    pub codec: String,
    pub codec_descriptor: String,
    pub width: u64,
    pub height: u64,
    pub bitrate: u64,
    pub frame_rate: f64,
    pub hdr: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr_metadata_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color_gamut: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_function: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AudioCapabilityRequest {
    pub stream_index: i64,
    pub content_type: String,
    pub codec: String,
    pub codec_descriptor: String,
    pub channels: u64,
    pub bitrate: u64,
    pub sample_rate: u64,
}

pub fn capability_request(
    stream: &Stream,
    width: u64,
    height: u64,
    bitrate: u64,
    frame_rate: f64,
) -> Option<VideoCapabilityRequest> {
    let codec_descriptor = codec_descriptor(stream)?;
    Some(VideoCapabilityRequest {
        content_type: format!("video/mp4; codecs=\"{codec_descriptor}\""),
        codec: stream.codec_name.clone(),
        codec_descriptor,
        width,
        height,
        bitrate,
        frame_rate,
        hdr: is_hdr(stream),
        hdr_metadata_type: stream
            .side_data_list
            .iter()
            .any(|side_data| side_data.side_data_type == "Mastering display metadata")
            .then_some("smpteSt2086"),
        color_gamut: color_gamut(stream.color_primaries.as_deref()),
        transfer_function: transfer_function(stream.color_transfer.as_deref()),
    })
}

pub fn audio_capability_request(
    stream: &Stream,
    channels: u64,
    bitrate: u64,
    sample_rate: u64,
) -> Option<AudioCapabilityRequest> {
    let codec_descriptor = audio_codec_descriptor(stream)?;
    Some(AudioCapabilityRequest {
        stream_index: stream.index,
        content_type: format!("audio/mp4; codecs=\"{codec_descriptor}\""),
        codec: stream.codec_name.clone(),
        codec_descriptor,
        channels,
        bitrate,
        sample_rate,
    })
}

pub fn remux_supported(stream: &Stream) -> bool {
    matches!(stream.codec_name.as_str(), "av1" | "h264" | "avc1")
        && codec_descriptor(stream).is_some()
}

pub fn audio_remux_supported(stream: &Stream) -> bool {
    audio_codec_descriptor(stream).is_some()
}

pub fn audio_codec_descriptor(stream: &Stream) -> Option<String> {
    match stream.codec_name.as_str() {
        "aac" => aac_codec_descriptor(stream),
        "eac3" => Some("ec-3".into()),
        "ac3" => Some("ac-3".into()),
        _ => None,
    }
}

fn aac_codec_descriptor(stream: &Stream) -> Option<String> {
    let object_type = stream
        .extradata_bytes()
        .and_then(|bytes| bytes.first().map(|byte| byte >> 3))
        .filter(|object_type| *object_type > 0 && *object_type < 31)
        .or_else(|| match stream.profile.as_deref()? {
            "Main" => Some(1),
            "LC" => Some(2),
            "SSR" => Some(3),
            "LTP" => Some(4),
            "HE-AAC" => Some(5),
            "HE-AACv2" => Some(29),
            _ => None,
        })?;
    Some(format!("mp4a.40.{object_type}"))
}

pub fn has_exact_codec_configuration(stream: &Stream) -> bool {
    match stream.codec_name.as_str() {
        "av1" => stream
            .extradata_bytes()
            .is_some_and(|bytes| bytes.len() >= 3 && bytes[0] & 0x80 != 0),
        "h264" | "avc1" => stream
            .extradata_bytes()
            .is_some_and(|bytes| bytes.len() >= 4 && bytes[0] == 1),
        _ => true,
    }
}

pub fn codec_descriptor(stream: &Stream) -> Option<String> {
    match stream.codec_name.as_str() {
        "av1" => av1_codec_descriptor(stream),
        "h264" | "avc1" => h264_codec_descriptor(stream),
        _ => None,
    }
}

pub fn is_hdr(stream: &Stream) -> bool {
    matches!(
        stream.color_transfer.as_deref(),
        Some("smpte2084" | "arib-std-b67")
    )
}

pub fn hdr_peak_nits(stream: &Stream) -> Option<f64> {
    stream
        .side_data_list
        .iter()
        .find_map(|side_data| side_data.max_content.map(|value| value as f64))
        .or_else(|| {
            stream.side_data_list.iter().find_map(|side_data| {
                let value = side_data.max_luminance.as_deref()?;
                let (numerator, denominator) = value.split_once('/')?;
                let numerator = numerator.parse::<f64>().ok()?;
                let denominator = denominator.parse::<f64>().ok()?;
                (denominator > 0.0).then_some(numerator / denominator)
            })
        })
        .filter(|value| value.is_finite() && *value > 0.0)
}

fn av1_codec_descriptor(stream: &Stream) -> Option<String> {
    let config = stream
        .extradata_bytes()
        .filter(|bytes| bytes.len() >= 3 && bytes[0] & 0x80 != 0)
        .map(|bytes| {
            let profile = bytes[1] >> 5;
            let level = bytes[1] & 0x1f;
            let tier = if bytes[2] & 0x80 != 0 { 'H' } else { 'M' };
            let high_bitdepth = bytes[2] & 0x40 != 0;
            let twelve_bit = bytes[2] & 0x20 != 0;
            let monochrome = u8::from(bytes[2] & 0x10 != 0);
            let subsampling_x = u8::from(bytes[2] & 0x08 != 0);
            let subsampling_y = u8::from(bytes[2] & 0x04 != 0);
            let chroma_sample_position = bytes[2] & 0x03;
            let bit_depth = if profile == 2 && high_bitdepth && twelve_bit {
                12
            } else if high_bitdepth {
                10
            } else {
                8
            };
            (
                profile,
                level,
                tier,
                bit_depth,
                monochrome,
                subsampling_x,
                subsampling_y,
                chroma_sample_position,
            )
        })
        .or_else(|| av1_config_from_probe(stream))?;
    let (profile, level, tier, depth, mono, subsampling_x, subsampling_y, chroma_position) = config;
    let color_primaries = color_primaries_code(stream.color_primaries.as_deref());
    let transfer = transfer_code(stream.color_transfer.as_deref());
    let matrix = matrix_code(stream.color_space.as_deref());
    let full_range = u8::from(matches!(stream.color_range.as_deref(), Some("pc" | "jpeg")));

    Some(format!(
        "av01.{profile}.{level:02}{tier}.{depth:02}.{mono}.{subsampling_x}{subsampling_y}{chroma_position}.{color_primaries:02}.{transfer:02}.{matrix:02}.{full_range}"
    ))
}

fn av1_config_from_probe(stream: &Stream) -> Option<(u8, u8, char, u8, u8, u8, u8, u8)> {
    let profile = match stream.profile.as_deref()? {
        "Main" => 0,
        "High" => 1,
        "Professional" => 2,
        _ => return None,
    };
    let level = u8::try_from(stream.level?).ok()?;
    let pix_fmt = stream.pix_fmt.as_deref()?;
    let bit_depth = if pix_fmt.contains("12") {
        12
    } else if pix_fmt.contains("10") {
        10
    } else {
        8
    };
    let monochrome = u8::from(pix_fmt.starts_with("gray"));
    let (subsampling_x, subsampling_y) = if monochrome == 1 {
        (1, 1)
    } else if pix_fmt.contains("420") {
        (1, 1)
    } else if pix_fmt.contains("422") {
        (1, 0)
    } else if pix_fmt.contains("444") || pix_fmt.starts_with("gbr") {
        (0, 0)
    } else {
        return None;
    };
    let chroma_position = match stream.chroma_location.as_deref() {
        Some("left") => 1,
        Some("topleft") | Some("top") => 2,
        _ => 0,
    };
    Some((
        profile,
        level,
        'M',
        bit_depth,
        monochrome,
        subsampling_x,
        subsampling_y,
        chroma_position,
    ))
}

fn h264_codec_descriptor(stream: &Stream) -> Option<String> {
    if let Some(bytes) = stream
        .extradata_bytes()
        .filter(|bytes| bytes.len() >= 4 && bytes[0] == 1)
    {
        return Some(format!(
            "avc1.{:02x}{:02x}{:02x}",
            bytes[1], bytes[2], bytes[3]
        ));
    }
    let profile_and_constraints = match stream.profile.as_deref()? {
        "Constrained Baseline" => "42e0",
        "Baseline" => "4200",
        "Main" => "4d00",
        "High" => "6400",
        "High 10" => "6e00",
        "High 4:2:2" => "7a00",
        "High 4:4:4 Predictive" => "f400",
        _ => return None,
    };
    let level = u8::try_from(stream.level?).ok()?;
    Some(format!("avc1.{profile_and_constraints}{level:02x}"))
}

fn color_primaries_code(value: Option<&str>) -> u8 {
    match value {
        Some("bt709") => 1,
        Some("bt470m") => 4,
        Some("bt470bg") => 5,
        Some("smpte170m") => 6,
        Some("smpte240m") => 7,
        Some("film") => 8,
        Some("bt2020") => 9,
        Some("smpte428") => 10,
        Some("smpte431") => 11,
        Some("smpte432") => 12,
        _ => 2,
    }
}

fn transfer_code(value: Option<&str>) -> u8 {
    match value {
        Some("bt709") => 1,
        Some("gamma22") => 4,
        Some("gamma28") => 5,
        Some("smpte170m") => 6,
        Some("smpte240m") => 7,
        Some("linear") => 8,
        Some("iec61966-2-4") => 11,
        Some("iec61966-2-1") => 13,
        Some("bt2020-10") => 14,
        Some("bt2020-12") => 15,
        Some("smpte2084") => 16,
        Some("smpte428") => 17,
        Some("arib-std-b67") => 18,
        _ => 2,
    }
}

fn matrix_code(value: Option<&str>) -> u8 {
    match value {
        Some("gbr") => 0,
        Some("bt709") => 1,
        Some("fcc") => 4,
        Some("bt470bg") => 5,
        Some("smpte170m") => 6,
        Some("smpte240m") => 7,
        Some("ycgco") => 8,
        Some("bt2020nc") => 9,
        Some("bt2020c") => 10,
        Some("smpte2085") => 11,
        Some("chroma-derived-nc") => 12,
        Some("chroma-derived-c") => 13,
        Some("ictcp") => 14,
        _ => 2,
    }
}

fn color_gamut(value: Option<&str>) -> Option<&'static str> {
    match value {
        Some("bt2020") => Some("rec2020"),
        Some("smpte432") => Some("p3"),
        Some("bt709") => Some("srgb"),
        _ => None,
    }
}

fn transfer_function(value: Option<&str>) -> Option<&'static str> {
    match value {
        Some("smpte2084") => Some("pq"),
        Some("arib-std-b67") => Some("hlg"),
        Some("bt709" | "iec61966-2-1") => Some("srgb"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn av1(extradata: &str, colors: (&str, &str, &str), chroma: Option<&str>) -> Stream {
        Stream {
            codec_name: "av1".into(),
            codec_type: "video".into(),
            profile: Some("Main".into()),
            level: Some(8),
            pix_fmt: Some("yuv420p10le".into()),
            color_range: Some("tv".into()),
            color_primaries: Some(colors.0.into()),
            color_transfer: Some(colors.1.into()),
            color_space: Some(colors.2.into()),
            chroma_location: chroma.map(str::to_owned),
            extradata: Some(extradata.into()),
            ..Default::default()
        }
    }

    #[test]
    fn derives_friday_descriptor_from_av1c() {
        let stream = av1(
            "\n00000000: 8108 4d00 0a0e 0000 0042 abbf c370 0be1\n",
            ("bt709", "bt709", "bt709"),
            Some("left"),
        );
        assert_eq!(
            codec_descriptor(&stream).as_deref(),
            Some("av01.0.08M.10.0.111.01.01.01.0")
        );
    }

    #[test]
    fn derives_matrix_descriptor_and_hdr_request_from_av1c() {
        let mut stream = av1(
            "\n00000000: 810c 4c00 0a0f 0000 0062 ebbf f1f9 d5f0\n",
            ("bt2020", "smpte2084", "bt2020nc"),
            None,
        );
        stream.side_data_list.push(super::super::ffprobe::SideData {
            side_data_type: "Mastering display metadata".into(),
            ..Default::default()
        });
        let request = capability_request(&stream, 3840, 1600, 11_618_576, 24000.0 / 1001.0)
            .expect("Matrix AV1 should have a capability request");
        assert_eq!(request.codec_descriptor, "av01.0.12M.10.0.110.09.16.09.0");
        assert!(request.hdr);
        assert_eq!(request.hdr_metadata_type, Some("smpteSt2086"));
        assert_eq!(request.color_gamut, Some("rec2020"));
        assert_eq!(request.transfer_function, Some("pq"));
    }

    #[test]
    fn falls_back_to_probe_fields_when_old_metadata_has_no_extradata() {
        let stream = av1("", ("bt709", "bt709", "bt709"), Some("left"));
        assert_eq!(
            codec_descriptor(&stream).as_deref(),
            Some("av01.0.08M.10.0.111.01.01.01.0")
        );
    }

    #[test]
    fn derives_h264_profile_compatibility_and_level_from_avcc() {
        let stream = Stream {
            codec_name: "h264".into(),
            codec_type: "video".into(),
            profile: Some("High".into()),
            level: Some(40),
            extradata: Some("\n00000000: 0164 0028 ffe1 0019\n".into()),
            ..Default::default()
        };
        assert!(has_exact_codec_configuration(&stream));
        assert_eq!(codec_descriptor(&stream).as_deref(), Some("avc1.640028"));
    }

    #[test]
    fn derives_matrix_eac3_capability_request() {
        let stream = Stream {
            index: 1,
            codec_name: "eac3".into(),
            codec_type: "audio".into(),
            channels: Some(6),
            channel_layout: Some("5.1(side)".into()),
            sample_rate: Some("48000".into()),
            bit_rate: Some("768000".into()),
            ..Default::default()
        };
        let request = audio_capability_request(&stream, 6, 768_000, 48_000).unwrap();
        assert_eq!(request.content_type, "audio/mp4; codecs=\"ec-3\"");
        assert_eq!(request.stream_index, 1);
        assert_eq!(request.channels, 6);
        assert!(audio_remux_supported(&stream));
    }

    #[test]
    fn aac_descriptor_uses_the_actual_audio_object_type() {
        let lc = Stream {
            codec_name: "aac".into(),
            codec_type: "audio".into(),
            extradata: Some("\n00000000: 1210\n".into()),
            ..Default::default()
        };
        assert_eq!(audio_codec_descriptor(&lc).as_deref(), Some("mp4a.40.2"));

        let he = Stream {
            codec_name: "aac".into(),
            codec_type: "audio".into(),
            profile: Some("HE-AAC".into()),
            ..Default::default()
        };
        assert_eq!(audio_codec_descriptor(&he).as_deref(), Some("mp4a.40.5"));
    }
}
