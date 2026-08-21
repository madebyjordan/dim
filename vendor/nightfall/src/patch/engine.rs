use crate::error::NightfallError;
use crate::patch::segment::Segment;
use crate::Result;
use std::convert::{TryFrom, TryInto};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

pub(crate) const COPY_BUFFER_BYTES: usize = 1024 * 1024;
const MAX_FRAGMENT_BYTES: u64 = 64 * 1024 * 1024 * 1024;
const MAX_CONTROL_BOX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_BRAND_BOX_BYTES: u64 = 64 * 1024;
const MAX_BOX_COUNT: usize = 16_384;
const MAX_BOX_DEPTH: usize = 8;
const MAX_SEGMENT_COUNT: usize = 4_096;
const MAX_SAMPLE_COUNT: u32 = 1_000_000;
const DEFAULT_STYP: [u8; 16] = [0, 0, 0, 16, b's', b't', b'y', b'p', 0, 0, 0, 0, 0, 0, 0, 0];

const FTYP: [u8; 4] = *b"ftyp";
const MDAT: [u8; 4] = *b"mdat";
const MFHD: [u8; 4] = *b"mfhd";
const MOOF: [u8; 4] = *b"moof";
const MOOV: [u8; 4] = *b"moov";
const SIDX: [u8; 4] = *b"sidx";
const STYP: [u8; 4] = *b"styp";
const TFDT: [u8; 4] = *b"tfdt";
const TFHD: [u8; 4] = *b"tfhd";
const TRAF: [u8; 4] = *b"traf";
const TRUN: [u8; 4] = *b"trun";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BoxSpan {
    start: u64,
    end: u64,
    header_bytes: u64,
    kind: [u8; 4],
}

impl BoxSpan {
    fn size(self) -> u64 {
        self.end - self.start
    }

    fn content_start(self) -> u64 {
        self.start + self.header_bytes
    }

    fn content_size(self) -> u64 {
        self.size() - self.header_bytes
    }
}

#[derive(Default)]
struct ParseBudget {
    boxes: usize,
}

#[derive(Clone, Copy)]
struct VersionedField {
    offset: u64,
    width: usize,
}

#[derive(Clone, Copy)]
struct MoofPlan {
    span: BoxSpan,
    sequence_offset: u64,
    first_tfdt: Option<VersionedField>,
}

#[derive(Default)]
struct SegmentBuilder {
    styp: Option<BoxSpan>,
    sidx: Option<(BoxSpan, u64)>,
    moof: Option<MoofPlan>,
}

impl SegmentBuilder {
    fn is_empty(&self) -> bool {
        self.styp.is_none() && self.sidx.is_none() && self.moof.is_none()
    }

    fn is_styp_only(&self) -> bool {
        self.styp.is_some() && self.sidx.is_none() && self.moof.is_none()
    }

    fn finish(self, mdat: BoxSpan) -> Result<SegmentPlan> {
        let moof = self.moof.ok_or(NightfallError::MissingSegmentBox)?;
        Ok(SegmentPlan {
            styp: self.styp,
            sidx: self.sidx,
            moof,
            mdat,
        })
    }
}

struct SegmentPlan {
    styp: Option<BoxSpan>,
    sidx: Option<(BoxSpan, u64)>,
    moof: MoofPlan,
    mdat: BoxSpan,
}

struct MediaPlan {
    segments: Vec<SegmentPlan>,
    next_sequence: u32,
}

struct InitPlan {
    ftyp: BoxSpan,
    moov: BoxSpan,
    moov_extras: Vec<BoxSpan>,
    segments: Vec<SegmentPlan>,
    next_sequence: u32,
}

#[derive(Clone, Copy)]
struct BytePatch {
    offset: u64,
    bytes: [u8; 8],
    len: usize,
}

impl BytePatch {
    fn u32(offset: u64, value: u32) -> Self {
        let mut bytes = [0_u8; 8];
        bytes[..4].copy_from_slice(&value.to_be_bytes());
        Self {
            offset,
            bytes,
            len: 4,
        }
    }

    fn u64(offset: u64, value: u64) -> Self {
        Self {
            offset,
            bytes: value.to_be_bytes(),
            len: 8,
        }
    }

    fn fourcc(offset: u64, value: [u8; 4]) -> Self {
        let mut bytes = [0_u8; 8];
        bytes[..4].copy_from_slice(&value);
        Self {
            offset,
            bytes,
            len: 4,
        }
    }
}

pub(crate) fn patch_media(input: &Path, output: &Path, sequence: u32) -> Result<u32> {
    let mut source = File::open(input)?;
    let plan = parse_media(&mut source, sequence)?;
    super::write_new_atomically(output, |destination| {
        write_segments(&mut source, destination, &plan.segments, sequence, None)
    })?;
    Ok(plan.next_sequence)
}

pub(crate) fn patch_init(
    input: &Path,
    media_output: &Path,
    normalized_output: &Path,
    sequence: u32,
) -> Result<u32> {
    let mut source = File::open(input)?;
    let plan = parse_init(&mut source, sequence)?;

    super::write_new_atomically(normalized_output, |destination| {
        write_normalized_init(&mut source, destination, &plan)
    })?;
    super::write_new_atomically(media_output, |destination| {
        write_segments(
            &mut source,
            destination,
            &plan.segments,
            sequence,
            Some(plan.ftyp),
        )
    })?;
    Ok(plan.next_sequence)
}

fn parse_media(source: &mut File, sequence: u32) -> Result<MediaPlan> {
    let file_end = validate_file_size(source)?;
    let mut budget = ParseBudget::default();
    let mut builder = SegmentBuilder::default();
    let mut segments = Vec::new();
    source.seek(SeekFrom::Start(0))?;

    while source.stream_position()? < file_end {
        let span = read_box_header(source, file_end, 0, &mut budget)?;
        match span.kind {
            STYP => {
                validate_brand_box(source, span)?;
                set_once(&mut builder.styp, span, "styp")?;
            }
            SIDX => {
                let earliest = validate_sidx(source, span, file_end)?;
                set_once(&mut builder.sidx, (span, earliest), "sidx")?;
            }
            MOOF => {
                let moof = parse_moof(source, span, &mut budget)?;
                set_once(&mut builder.moof, moof, "moof")?;
            }
            MDAT => {
                ensure_segment_capacity(segments.len())?;
                segments.push(std::mem::take(&mut builder).finish(span)?);
            }
            _ => {}
        }
        source.seek(SeekFrom::Start(span.end))?;
    }

    if !builder.is_empty() {
        if segments.is_empty() && builder.is_styp_only() {
            return Err(NightfallError::PartialSegment(
                Segment::default().gen_styp(),
            ));
        }
        return Err(NightfallError::MissingSegmentBox);
    }
    if segments.is_empty() {
        return Err(NightfallError::MissingSegmentBox);
    }

    validate_transform_widths(&segments, false)?;
    let next_sequence = advance_sequence(sequence, segments.len())?;
    Ok(MediaPlan {
        segments,
        next_sequence,
    })
}

fn parse_init(source: &mut File, sequence: u32) -> Result<InitPlan> {
    let file_end = validate_file_size(source)?;
    let mut budget = ParseBudget::default();
    let mut ftyp = None;
    let mut moov = None;
    let mut moov_extras = Vec::new();
    let mut builder = SegmentBuilder::default();
    let mut segments = Vec::new();
    source.seek(SeekFrom::Start(0))?;

    while source.stream_position()? < file_end {
        let span = read_box_header(source, file_end, 0, &mut budget)?;
        match span.kind {
            FTYP => {
                if !builder.is_empty() || !segments.is_empty() {
                    return Err(invalid("ftyp appears after embedded media"));
                }
                validate_brand_box(source, span)?;
                set_once(&mut ftyp, span, "ftyp")?;
            }
            MOOV => {
                if !builder.is_empty() || !segments.is_empty() {
                    return Err(invalid("moov appears after embedded media"));
                }
                validate_control_box(span, "moov")?;
                set_once(&mut moov, span, "moov")?;
            }
            STYP => {
                validate_brand_box(source, span)?;
                set_once(&mut builder.styp, span, "styp")?;
            }
            SIDX => {
                let earliest = validate_sidx(source, span, file_end)?;
                set_once(&mut builder.sidx, (span, earliest), "sidx")?;
            }
            MOOF => {
                let parsed = parse_moof(source, span, &mut budget)?;
                set_once(&mut builder.moof, parsed, "moof")?;
            }
            MDAT => {
                ensure_segment_capacity(segments.len())?;
                segments.push(std::mem::take(&mut builder).finish(span)?);
            }
            _ => moov_extras.push(span),
        }
        source.seek(SeekFrom::Start(span.end))?;
    }

    if !builder.is_empty() {
        return Err(invalid(
            "initialization segment ends with incomplete embedded media",
        ));
    }
    if segments.is_empty() {
        return Err(NightfallError::MissingSegmentBox);
    }
    let ftyp = ftyp.ok_or_else(|| invalid("initialization segment is missing ftyp"))?;
    let moov = moov.ok_or_else(|| invalid("initialization segment is missing moov"))?;
    let normalized_moov_size = normalized_moov_size(moov, &moov_extras)?;
    if normalized_moov_size > MAX_CONTROL_BOX_BYTES {
        return Err(invalid(format!(
            "normalized moov size {normalized_moov_size} exceeds limit {MAX_CONTROL_BOX_BYTES}"
        )));
    }

    validate_transform_widths(&segments, true)?;
    let next_sequence = advance_sequence(sequence, segments.len())?;
    Ok(InitPlan {
        ftyp,
        moov,
        moov_extras,
        segments,
        next_sequence,
    })
}

fn validate_file_size(source: &File) -> Result<u64> {
    let size = source.metadata()?.len();
    validate_fragment_size(size)?;
    Ok(size)
}

fn validate_fragment_size(size: u64) -> Result<()> {
    if size == 0 {
        return Err(invalid("fragment is empty"));
    }
    if size > MAX_FRAGMENT_BYTES {
        return Err(invalid(format!(
            "fragment size {size} exceeds limit {MAX_FRAGMENT_BYTES}"
        )));
    }
    Ok(())
}

fn read_box_header(
    source: &mut File,
    parent_end: u64,
    depth: usize,
    budget: &mut ParseBudget,
) -> Result<BoxSpan> {
    if depth > MAX_BOX_DEPTH {
        return Err(invalid(format!(
            "box nesting depth {depth} exceeds limit {MAX_BOX_DEPTH}"
        )));
    }
    budget.boxes = budget
        .boxes
        .checked_add(1)
        .ok_or_else(|| invalid("box count overflow"))?;
    if budget.boxes > MAX_BOX_COUNT {
        return Err(invalid(format!("box count exceeds limit {MAX_BOX_COUNT}")));
    }

    let start = source.stream_position()?;
    let remaining = parent_end
        .checked_sub(start)
        .ok_or_else(|| invalid("box offset exceeds parent boundary"))?;
    if remaining < 8 {
        return Err(invalid(format!(
            "truncated box header at offset {start}: {remaining} bytes remain"
        )));
    }

    let header = read_array::<8>(source, start, "box header")?;
    let short_size = u32::from_be_bytes(header[..4].try_into().expect("four-byte slice"));
    let kind = header[4..8].try_into().expect("four-byte slice");
    let (size, header_bytes) = match short_size {
        0 => (remaining, 8),
        1 => {
            if remaining < 16 {
                return Err(invalid(format!(
                    "truncated extended box header at offset {start}"
                )));
            }
            let extended = read_array::<8>(source, start + 8, "extended box size")?;
            (u64::from_be_bytes(extended), 16)
        }
        value => (u64::from(value), 8),
    };
    if size < header_bytes {
        return Err(invalid(format!(
            "box {} at offset {start} has size {size} smaller than its {header_bytes}-byte header",
            fourcc(kind)
        )));
    }
    let end = start
        .checked_add(size)
        .ok_or_else(|| invalid(format!("box {} offset overflow", fourcc(kind))))?;
    if end > parent_end {
        return Err(invalid(format!(
            "box {} at offset {start} ends at {end}, past parent boundary {parent_end}",
            fourcc(kind)
        )));
    }
    Ok(BoxSpan {
        start,
        end,
        header_bytes,
        kind,
    })
}

fn validate_control_box(span: BoxSpan, name: &str) -> Result<()> {
    if span.size() > MAX_CONTROL_BOX_BYTES {
        return Err(invalid(format!(
            "{name} size {} exceeds control-box limit {MAX_CONTROL_BOX_BYTES}",
            span.size()
        )));
    }
    Ok(())
}

fn validate_brand_box(source: &mut File, span: BoxSpan) -> Result<()> {
    if span.size() > MAX_BRAND_BOX_BYTES {
        return Err(invalid(format!(
            "{} size {} exceeds brand-box limit {MAX_BRAND_BOX_BYTES}",
            fourcc(span.kind),
            span.size()
        )));
    }
    let content_size = span.content_size();
    if content_size < 8 || !(content_size - 8).is_multiple_of(4) {
        return Err(invalid(format!(
            "{} has invalid brand-field width {content_size}",
            fourcc(span.kind)
        )));
    }
    // Force an exact bounded read of the mandatory major-brand and minor-version fields.
    let _ = read_array::<8>(source, span.content_start(), "brand fields")?;
    Ok(())
}

fn validate_sidx(source: &mut File, span: BoxSpan, file_end: u64) -> Result<u64> {
    validate_control_box(span, "sidx")?;
    let content = span.content_start();
    let (version, _) = read_full_box(source, span, "sidx")?;
    let (base_size, earliest_offset, first_offset_offset, count_offset) = match version {
        0 => (24_u64, 12_u64, 16_u64, 22_u64),
        1 => (32_u64, 12_u64, 20_u64, 30_u64),
        _ => {
            return Err(invalid(format!(
                "sidx has unsupported version {version}; expected 0 or 1"
            )))
        }
    };
    if span.content_size() < base_size {
        return Err(invalid(format!(
            "version-{version} sidx is truncated: {} content bytes, need at least {base_size}",
            span.content_size()
        )));
    }
    let timescale = read_u32(
        source,
        checked_add(content, 8, "sidx timescale offset")?,
        "sidx timescale",
    )?;
    if timescale == 0 {
        return Err(invalid("sidx timescale is zero"));
    }
    let earliest = if version == 0 {
        u64::from(read_u32(
            source,
            checked_add(content, earliest_offset, "sidx EPT offset")?,
            "sidx earliest presentation time",
        )?)
    } else {
        read_u64(
            source,
            checked_add(content, earliest_offset, "sidx EPT offset")?,
            "sidx earliest presentation time",
        )?
    };
    let first_offset = if version == 0 {
        u64::from(read_u32(
            source,
            checked_add(content, first_offset_offset, "sidx first-offset offset")?,
            "sidx first offset",
        )?)
    } else {
        read_u64(
            source,
            checked_add(content, first_offset_offset, "sidx first-offset offset")?,
            "sidx first offset",
        )?
    };
    let reference_count = u64::from(read_u16(
        source,
        checked_add(content, count_offset, "sidx reference-count offset")?,
        "sidx reference count",
    )?);
    let references_size = reference_count
        .checked_mul(12)
        .ok_or_else(|| invalid("sidx reference table size overflow"))?;
    let expected_size = base_size
        .checked_add(references_size)
        .ok_or_else(|| invalid("sidx content size overflow"))?;
    if span.content_size() != expected_size {
        return Err(invalid(format!(
            "version-{version} sidx has {} content bytes; fields require {expected_size}",
            span.content_size()
        )));
    }

    let mut referenced_end = span
        .end
        .checked_add(first_offset)
        .ok_or_else(|| invalid("sidx first offset overflows file position"))?;
    for index in 0..reference_count {
        let entry_offset = checked_add(
            content,
            base_size
                .checked_add(
                    index
                        .checked_mul(12)
                        .ok_or_else(|| invalid("sidx reference index overflow"))?,
                )
                .ok_or_else(|| invalid("sidx reference offset overflow"))?,
            "sidx reference offset",
        )?;
        let reference = read_u32(source, entry_offset, "sidx reference")?;
        referenced_end = referenced_end
            .checked_add(u64::from(reference & 0x7fff_ffff))
            .ok_or_else(|| invalid("sidx referenced-size arithmetic overflow"))?;
    }
    if referenced_end > file_end {
        return Err(invalid(format!(
            "sidx references through offset {referenced_end}, past fragment end {file_end}"
        )));
    }
    Ok(earliest)
}

fn parse_moof(source: &mut File, span: BoxSpan, budget: &mut ParseBudget) -> Result<MoofPlan> {
    validate_control_box(span, "moof")?;
    let mut sequence_offset = None;
    let mut first_tfdt = None;
    let mut traf_index = 0_usize;
    source.seek(SeekFrom::Start(span.content_start()))?;

    while source.stream_position()? < span.end {
        let child = read_box_header(source, span.end, 1, budget)?;
        match child.kind {
            MFHD => {
                let offset = validate_mfhd(source, child)?;
                set_once(&mut sequence_offset, offset, "mfhd")?;
            }
            TRAF => {
                let tfdt = parse_traf(source, child, budget)?;
                if traf_index == 0 {
                    first_tfdt = tfdt;
                }
                traf_index = traf_index
                    .checked_add(1)
                    .ok_or_else(|| invalid("traf count overflow"))?;
            }
            _ => {}
        }
        source.seek(SeekFrom::Start(child.end))?;
    }
    Ok(MoofPlan {
        span,
        sequence_offset: sequence_offset
            .ok_or_else(|| invalid("moof is missing mandatory mfhd"))?,
        first_tfdt,
    })
}

fn validate_mfhd(source: &mut File, span: BoxSpan) -> Result<u64> {
    let (version, _) = read_full_box(source, span, "mfhd")?;
    if version != 0 {
        return Err(invalid(format!(
            "mfhd has unsupported version {version}; expected 0"
        )));
    }
    if span.content_size() != 8 {
        return Err(invalid(format!(
            "mfhd has {} content bytes; expected 8",
            span.content_size()
        )));
    }
    checked_add(span.content_start(), 4, "mfhd sequence offset")
}

fn parse_traf(
    source: &mut File,
    span: BoxSpan,
    budget: &mut ParseBudget,
) -> Result<Option<VersionedField>> {
    validate_control_box(span, "traf")?;
    let mut has_tfhd = false;
    let mut tfdt = None;
    source.seek(SeekFrom::Start(span.content_start()))?;
    while source.stream_position()? < span.end {
        let child = read_box_header(source, span.end, 2, budget)?;
        match child.kind {
            TFHD => {
                if has_tfhd {
                    return Err(invalid("traf contains duplicate tfhd boxes"));
                }
                validate_tfhd(source, child)?;
                has_tfhd = true;
            }
            TFDT => {
                let parsed = validate_tfdt(source, child)?;
                set_once(&mut tfdt, parsed, "tfdt")?;
            }
            TRUN => validate_trun(source, child)?,
            _ => {}
        }
        source.seek(SeekFrom::Start(child.end))?;
    }
    if !has_tfhd {
        return Err(invalid("traf is missing mandatory tfhd"));
    }
    Ok(tfdt)
}

fn validate_tfhd(source: &mut File, span: BoxSpan) -> Result<()> {
    let (version, flags) = read_full_box(source, span, "tfhd")?;
    if version != 0 {
        return Err(invalid(format!(
            "tfhd has unsupported version {version}; expected 0"
        )));
    }
    const ALLOWED_FLAGS: u32 = 0x03003b;
    if flags & !ALLOWED_FLAGS != 0 {
        return Err(invalid(format!("tfhd has unsupported flags {flags:#08x}")));
    }
    let mut expected = 8_u64;
    if flags & 0x000001 != 0 {
        expected = checked_add(expected, 8, "tfhd base-data-offset width")?;
    }
    for flag in [0x000002, 0x000008, 0x000010, 0x000020] {
        if flags & flag != 0 {
            expected = checked_add(expected, 4, "tfhd optional-field width")?;
        }
    }
    if span.content_size() != expected {
        return Err(invalid(format!(
            "tfhd has {} content bytes; flags require {expected}",
            span.content_size()
        )));
    }
    Ok(())
}

fn validate_tfdt(source: &mut File, span: BoxSpan) -> Result<VersionedField> {
    let (version, _) = read_full_box(source, span, "tfdt")?;
    let width = match version {
        0 => 4,
        1 => 8,
        _ => {
            return Err(invalid(format!(
                "tfdt has unsupported version {version}; expected 0 or 1"
            )))
        }
    };
    let expected = 4_u64 + u64::try_from(width).expect("field width fits u64");
    if span.content_size() != expected {
        return Err(invalid(format!(
            "version-{version} tfdt has {} content bytes; expected {expected}",
            span.content_size()
        )));
    }
    Ok(VersionedField {
        offset: checked_add(span.content_start(), 4, "tfdt value offset")?,
        width,
    })
}

fn validate_trun(source: &mut File, span: BoxSpan) -> Result<()> {
    let (version, flags) = read_full_box(source, span, "trun")?;
    if version > 1 {
        return Err(invalid(format!(
            "trun has unsupported version {version}; expected 0 or 1"
        )));
    }
    const ALLOWED_FLAGS: u32 = 0x000f05;
    if flags & !ALLOWED_FLAGS != 0 {
        return Err(invalid(format!("trun has unsupported flags {flags:#08x}")));
    }
    if span.content_size() < 8 {
        return Err(invalid("trun is truncated before sample_count"));
    }
    let sample_count = read_u32(
        source,
        checked_add(span.content_start(), 4, "trun sample-count offset")?,
        "trun sample count",
    )?;
    if sample_count > MAX_SAMPLE_COUNT {
        return Err(invalid(format!(
            "trun sample count {sample_count} exceeds limit {MAX_SAMPLE_COUNT}"
        )));
    }
    let mut fixed = 8_u64;
    if flags & 0x000001 != 0 {
        fixed = checked_add(fixed, 4, "trun data-offset width")?;
    }
    if flags & 0x000004 != 0 {
        fixed = checked_add(fixed, 4, "trun first-sample-flags width")?;
    }
    let per_sample_fields = [0x000100, 0x000200, 0x000400, 0x000800]
        .iter()
        .filter(|flag| flags & **flag != 0)
        .count();
    let per_sample = u64::try_from(per_sample_fields)
        .expect("field count fits u64")
        .checked_mul(4)
        .ok_or_else(|| invalid("trun per-sample width overflow"))?;
    let samples_size = u64::from(sample_count)
        .checked_mul(per_sample)
        .ok_or_else(|| invalid("trun sample table size overflow"))?;
    let expected = fixed
        .checked_add(samples_size)
        .ok_or_else(|| invalid("trun content size overflow"))?;
    if span.content_size() != expected {
        return Err(invalid(format!(
            "trun has {} content bytes; flags and sample_count require {expected}",
            span.content_size()
        )));
    }
    Ok(())
}

fn read_full_box(source: &mut File, span: BoxSpan, name: &str) -> Result<(u8, u32)> {
    if span.content_size() < 4 {
        return Err(invalid(format!("{name} is truncated before version/flags")));
    }
    let bytes = read_array::<4>(source, span.content_start(), "full-box header")?;
    let flags = (u32::from(bytes[1]) << 16) | (u32::from(bytes[2]) << 8) | u32::from(bytes[3]);
    Ok((bytes[0], flags))
}

fn write_segments(
    source: &mut File,
    destination: &mut File,
    segments: &[SegmentPlan],
    sequence: u32,
    init_ftyp: Option<BoxSpan>,
) -> Result<()> {
    let mut current_sequence = sequence;
    for segment in segments {
        if let Some(styp) = segment.styp {
            copy_span(source, destination, styp)?;
        } else if let Some(ftyp) = init_ftyp {
            copy_span_with_patches(
                source,
                destination,
                ftyp,
                &[BytePatch::fourcc(ftyp.start + 4, STYP)],
            )?;
        } else {
            destination.write_all(&DEFAULT_STYP)?;
        }
        if let Some((sidx, _)) = segment.sidx {
            copy_span(source, destination, sidx)?;
        }

        let mut patches = Vec::with_capacity(2);
        patches.push(BytePatch::u32(
            segment.moof.sequence_offset,
            current_sequence,
        ));
        if let (Some((_, earliest)), Some(field)) = (segment.sidx, segment.moof.first_tfdt) {
            patches.push(match field.width {
                4 => BytePatch::u32(
                    field.offset,
                    u32::try_from(earliest).map_err(|_| {
                        invalid(format!(
                            "SIDX presentation time {earliest} does not fit version-0 tfdt"
                        ))
                    })?,
                ),
                8 => BytePatch::u64(field.offset, earliest),
                _ => return Err(invalid("unsupported tfdt patch width")),
            });
        }
        copy_span_with_patches(source, destination, segment.moof.span, &patches)?;
        copy_span(source, destination, segment.mdat)?;
        current_sequence = current_sequence
            .checked_add(1)
            .ok_or_else(|| invalid("segment sequence number overflow"))?;
    }
    Ok(())
}

fn validate_transform_widths(segments: &[SegmentPlan], normalize_dts: bool) -> Result<()> {
    if !normalize_dts {
        return Ok(());
    }
    for segment in segments {
        if let (Some((_, earliest)), Some(field)) = (segment.sidx, segment.moof.first_tfdt) {
            match field.width {
                4 => {
                    u32::try_from(earliest).map_err(|_| {
                        invalid(format!(
                            "SIDX presentation time {earliest} does not fit version-0 tfdt"
                        ))
                    })?;
                }
                8 => {}
                _ => return Err(invalid("unsupported tfdt patch width")),
            }
        }
    }
    Ok(())
}

fn write_normalized_init(source: &mut File, destination: &mut File, plan: &InitPlan) -> Result<()> {
    copy_span(source, destination, plan.ftyp)?;
    let moov_size = normalized_moov_size(plan.moov, &plan.moov_extras)?;
    write_standard_header(destination, moov_size, MOOV)?;
    copy_exact(
        source,
        destination,
        plan.moov.content_start(),
        plan.moov.content_size(),
    )?;
    for extra in &plan.moov_extras {
        copy_span(source, destination, *extra)?;
    }
    Ok(())
}

fn normalized_moov_size(moov: BoxSpan, extras: &[BoxSpan]) -> Result<u64> {
    let mut size = 8_u64
        .checked_add(moov.content_size())
        .ok_or_else(|| invalid("normalized moov size overflow"))?;
    for extra in extras {
        size = size
            .checked_add(extra.size())
            .ok_or_else(|| invalid("normalized moov extras overflow"))?;
    }
    if size > u64::from(u32::MAX) {
        return Err(invalid("normalized moov requires an extended header"));
    }
    Ok(size)
}

fn write_standard_header(destination: &mut File, size: u64, kind: [u8; 4]) -> Result<()> {
    let size = u32::try_from(size).map_err(|_| invalid("box size exceeds 32-bit header"))?;
    destination.write_all(&size.to_be_bytes())?;
    destination.write_all(&kind)?;
    Ok(())
}

fn copy_span(source: &mut File, destination: &mut File, span: BoxSpan) -> Result<()> {
    copy_exact(source, destination, span.start, span.size())
}

fn copy_span_with_patches(
    source: &mut File,
    destination: &mut File,
    span: BoxSpan,
    patches: &[BytePatch],
) -> Result<()> {
    let mut patches = patches.to_vec();
    patches.sort_by_key(|patch| patch.offset);
    let mut cursor = span.start;
    for patch in patches {
        let patch_end = patch
            .offset
            .checked_add(u64::try_from(patch.len).expect("patch width fits u64"))
            .ok_or_else(|| invalid("patch offset overflow"))?;
        if patch.offset < cursor || patch_end > span.end {
            return Err(invalid(
                "patch is outside its source box or overlaps another patch",
            ));
        }
        copy_exact(source, destination, cursor, patch.offset - cursor)?;
        destination.write_all(&patch.bytes[..patch.len])?;
        cursor = patch_end;
    }
    copy_exact(source, destination, cursor, span.end - cursor)
}

fn copy_exact(
    source: &mut File,
    destination: &mut File,
    offset: u64,
    mut bytes: u64,
) -> Result<()> {
    source.seek(SeekFrom::Start(offset))?;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    while bytes != 0 {
        let requested = usize::try_from(bytes.min(COPY_BUFFER_BYTES as u64))
            .expect("bounded copy request fits usize");
        source
            .read_exact(&mut buffer[..requested])
            .map_err(|error| {
                invalid(format!(
                    "source truncated while copying offset {}: {error}",
                    source.stream_position().unwrap_or(offset)
                ))
            })?;
        destination.write_all(&buffer[..requested])?;
        bytes -= requested as u64;
    }
    Ok(())
}

fn read_array<const N: usize>(source: &mut File, offset: u64, label: &str) -> Result<[u8; N]> {
    source.seek(SeekFrom::Start(offset))?;
    let mut bytes = [0_u8; N];
    source
        .read_exact(&mut bytes)
        .map_err(|error| invalid(format!("truncated {label} at offset {offset}: {error}")))?;
    Ok(bytes)
}

fn read_u16(source: &mut File, offset: u64, label: &str) -> Result<u16> {
    Ok(u16::from_be_bytes(read_array::<2>(source, offset, label)?))
}

fn read_u32(source: &mut File, offset: u64, label: &str) -> Result<u32> {
    Ok(u32::from_be_bytes(read_array::<4>(source, offset, label)?))
}

fn read_u64(source: &mut File, offset: u64, label: &str) -> Result<u64> {
    Ok(u64::from_be_bytes(read_array::<8>(source, offset, label)?))
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<()> {
    if slot.replace(value).is_some() {
        return Err(invalid(format!("duplicate {name} box")));
    }
    Ok(())
}

fn advance_sequence(sequence: u32, segment_count: usize) -> Result<u32> {
    let count = u32::try_from(segment_count).map_err(|_| invalid("segment count exceeds u32"))?;
    sequence
        .checked_add(count)
        .ok_or_else(|| invalid("segment sequence number overflow"))
}

fn ensure_segment_capacity(current_count: usize) -> Result<()> {
    if current_count >= MAX_SEGMENT_COUNT {
        return Err(invalid(format!(
            "segment count exceeds limit {MAX_SEGMENT_COUNT}"
        )));
    }
    Ok(())
}

fn checked_add(left: u64, right: u64, label: &str) -> Result<u64> {
    left.checked_add(right)
        .ok_or_else(|| invalid(format!("{label} overflow")))
}

fn fourcc(kind: [u8; 4]) -> String {
    kind.iter()
        .map(|byte| {
            if byte.is_ascii_graphic() {
                char::from(*byte)
            } else {
                '.'
            }
        })
        .collect()
}

fn invalid(message: impl Into<String>) -> NightfallError {
    NightfallError::InvalidFragment(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::{BufReader, Seek, Write};

    fn standard_box(kind: [u8; 4], content: &[u8]) -> Vec<u8> {
        let size = u32::try_from(content.len() + 8).expect("test box fits a short header");
        let mut bytes = Vec::with_capacity(size as usize);
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(content);
        bytes
    }

    fn extended_box(kind: [u8; 4], content: &[u8]) -> Vec<u8> {
        let size = u64::try_from(content.len() + 16).expect("test box fits u64");
        let mut bytes = Vec::with_capacity(size as usize);
        bytes.extend_from_slice(&1_u32.to_be_bytes());
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(&size.to_be_bytes());
        bytes.extend_from_slice(content);
        bytes
    }

    fn zero_box(kind: [u8; 4], content: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(content.len() + 8);
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(&kind);
        bytes.extend_from_slice(content);
        bytes
    }

    fn brand_box(kind: [u8; 4]) -> Vec<u8> {
        let mut content = Vec::new();
        content.extend_from_slice(b"isom");
        content.extend_from_slice(&0_u32.to_be_bytes());
        content.extend_from_slice(b"iso6");
        standard_box(kind, &content)
    }

    fn mfhd(sequence: u32) -> Vec<u8> {
        let mut content = vec![0, 0, 0, 0];
        content.extend_from_slice(&sequence.to_be_bytes());
        standard_box(MFHD, &content)
    }

    fn tfhd() -> Vec<u8> {
        let mut content = vec![0, 0, 0, 0];
        content.extend_from_slice(&1_u32.to_be_bytes());
        standard_box(TFHD, &content)
    }

    fn tfdt(version: u8, value: u64) -> Vec<u8> {
        let mut content = vec![version, 0, 0, 0];
        match version {
            0 => content.extend_from_slice(
                &u32::try_from(value)
                    .expect("version-0 test value fits u32")
                    .to_be_bytes(),
            ),
            _ => content.extend_from_slice(&value.to_be_bytes()),
        }
        standard_box(TFDT, &content)
    }

    fn moof(sequence: u32, decode_time: Option<(u8, u64)>) -> Vec<u8> {
        let mut content = mfhd(sequence);
        if let Some((version, value)) = decode_time {
            let mut traf = tfhd();
            traf.extend_from_slice(&tfdt(version, value));
            content.extend_from_slice(&standard_box(TRAF, &traf));
        }
        standard_box(MOOF, &content)
    }

    fn mdat(payload: &[u8]) -> Vec<u8> {
        standard_box(MDAT, payload)
    }

    fn sidx(version: u8, earliest: u64, referenced_size: u32) -> Vec<u8> {
        let mut content = vec![version, 0, 0, 0];
        content.extend_from_slice(&1_u32.to_be_bytes());
        content.extend_from_slice(&48_000_u32.to_be_bytes());
        match version {
            0 => {
                content.extend_from_slice(
                    &u32::try_from(earliest)
                        .expect("version-0 test EPT fits u32")
                        .to_be_bytes(),
                );
                content.extend_from_slice(&0_u32.to_be_bytes());
            }
            _ => {
                content.extend_from_slice(&earliest.to_be_bytes());
                content.extend_from_slice(&0_u64.to_be_bytes());
            }
        }
        content.extend_from_slice(&0_u16.to_be_bytes());
        content.extend_from_slice(&1_u16.to_be_bytes());
        content.extend_from_slice(&(referenced_size & 0x7fff_ffff).to_be_bytes());
        content.extend_from_slice(&48_000_u32.to_be_bytes());
        content.extend_from_slice(&0x9000_0000_u32.to_be_bytes());
        standard_box(SIDX, &content)
    }

    fn media_fragment(sequence: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = brand_box(STYP);
        bytes.extend_from_slice(&moof(sequence, None));
        bytes.extend_from_slice(&mdat(payload));
        bytes
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).expect("test fragment should be written");
    }

    fn type_position(bytes: &[u8], kind: [u8; 4]) -> usize {
        bytes
            .windows(4)
            .position(|window| window == kind)
            .expect("box type should exist")
    }

    fn patched_u32(bytes: &[u8], kind: [u8; 4]) -> u32 {
        let position = type_position(bytes, kind) + 8;
        u32::from_be_bytes(
            bytes[position..position + 4]
                .try_into()
                .expect("field has four bytes"),
        )
    }

    #[test]
    fn valid_media_is_patched_idempotently_without_changing_payload_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.m4s");
        let output = temp.path().join("published.m4s");
        let repeated = temp.path().join("repeated.m4s");
        let input_bytes = media_fragment(1, b"deterministic-payload");
        write(&input, &input_bytes);

        assert_eq!(patch_media(&input, &output, 77).unwrap(), 78);
        let first = fs::read(&output).unwrap();
        assert_eq!(patched_u32(&first, MFHD), 77);
        assert!(first.ends_with(b"deterministic-payload"));

        assert_eq!(patch_media(&input, &output, 77).unwrap(), 78);
        assert_eq!(fs::read(&output).unwrap(), first);
        assert_eq!(patch_media(&output, &repeated, 77).unwrap(), 78);
        assert_eq!(fs::read(repeated).unwrap(), first);
    }

    #[test]
    fn non_identical_existing_publication_is_never_replaced() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("input.m4s");
        let output = temp.path().join("published.m4s");
        write(&input, &media_fragment(1, b"source"));
        write(&output, b"existing-public-bytes");

        assert!(patch_media(&input, &output, 2).is_err());
        assert_eq!(fs::read(output).unwrap(), b"existing-public-bytes");
    }

    #[test]
    fn extended_headers_and_zero_sized_final_mdat_are_supported() {
        let temp = tempfile::tempdir().unwrap();
        let extended_input = temp.path().join("extended.m4s");
        let extended_output = temp.path().join("extended-output.m4s");
        let brand_content = &brand_box(STYP)[8..];
        let moof_content = &moof(1, None)[8..];
        let mut extended = extended_box(STYP, brand_content);
        extended.extend_from_slice(&extended_box(MOOF, moof_content));
        extended.extend_from_slice(&extended_box(MDAT, b"extended-payload"));
        write(&extended_input, &extended);
        assert_eq!(
            patch_media(&extended_input, &extended_output, 9).unwrap(),
            10
        );
        let output = fs::read(extended_output).unwrap();
        assert_eq!(&output[..4], &1_u32.to_be_bytes());
        assert_eq!(patched_u32(&output, MFHD), 9);
        assert!(output.ends_with(b"extended-payload"));

        let zero_input = temp.path().join("zero.m4s");
        let zero_output = temp.path().join("zero-output.m4s");
        let mut zero = brand_box(STYP);
        zero.extend_from_slice(&moof(1, None));
        zero.extend_from_slice(&zero_box(MDAT, b"to-end"));
        write(&zero_input, &zero);
        assert_eq!(patch_media(&zero_input, &zero_output, 11).unwrap(), 12);
        assert!(fs::read(zero_output).unwrap().ends_with(b"to-end"));
    }

    #[test]
    fn ffmpeg_version_one_sidx_bytes_and_tfdt_width_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("init.mp4");
        let media_output = temp.path().join("media.m4s");
        let normalized_output = temp.path().join("normalized.mp4");
        let fragment_moof = moof(1, Some((1, 3)));
        let fragment_mdat = mdat(b"payload");
        let raw_sidx = sidx(
            1,
            0x1_0000_0042,
            u32::try_from(fragment_moof.len() + fragment_mdat.len()).unwrap(),
        );
        let ftyp = brand_box(FTYP);
        let moov = standard_box(MOOV, &[]);
        let mut input_bytes = ftyp.clone();
        input_bytes.extend_from_slice(&moov);
        input_bytes.extend_from_slice(&raw_sidx);
        input_bytes.extend_from_slice(&fragment_moof);
        input_bytes.extend_from_slice(&fragment_mdat);
        write(&input, &input_bytes);

        assert_eq!(
            patch_init(&input, &media_output, &normalized_output, 31).unwrap(),
            32
        );
        let media = fs::read(media_output).unwrap();
        let sidx_type = type_position(&media, SIDX);
        let sidx_start = sidx_type - 4;
        assert_eq!(&media[sidx_start..sidx_start + raw_sidx.len()], &raw_sidx);
        assert_eq!(patched_u32(&media, MFHD), 31);
        let tfdt_type = type_position(&media, TFDT);
        assert_eq!(media[tfdt_type + 4], 1);
        assert_eq!(
            u64::from_be_bytes(media[tfdt_type + 8..tfdt_type + 16].try_into().unwrap()),
            0x1_0000_0042
        );
        assert_eq!(fs::read(normalized_output).unwrap(), [ftyp, moov].concat());
    }

    #[test]
    fn valid_version_one_fixture_matches_legacy_public_bytes() {
        use crate::patch::init_segment::InitSegment;

        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("init.mp4");
        let media_output = temp.path().join("media.m4s");
        let normalized_output = temp.path().join("normalized.mp4");
        let legacy_media = temp.path().join("legacy-media.m4s");
        let legacy_normalized = temp.path().join("legacy-normalized.mp4");
        // This mirrors FFmpeg's valid no-traf partial-init shape, for which the legacy adapter's
        // public serialization was already spec-conformant. TFDT normalization has its own
        // version-width regression above because the legacy mp4 crate rewrites tfhd widths.
        let fragment_moof = moof(1, None);
        let fragment_mdat = mdat(b"legacy-equivalence-payload");
        let raw_sidx = sidx(
            1,
            42,
            u32::try_from(fragment_moof.len() + fragment_mdat.len()).unwrap(),
        );
        let mut bytes = brand_box(FTYP);
        bytes.extend_from_slice(&standard_box(MOOV, &[]));
        bytes.extend_from_slice(&raw_sidx);
        bytes.extend_from_slice(&fragment_moof);
        bytes.extend_from_slice(&fragment_mdat);
        write(&input, &bytes);

        let file = File::open(&input).unwrap();
        let size = file.metadata().unwrap().len();
        let mut parsed = InitSegment::from_reader(BufReader::new(file), size).unwrap();
        let mut legacy_media_file = File::create(&legacy_media).unwrap();
        let mut sequence = 19;
        while let Some(segment) = parsed.segments.pop_front() {
            segment
                .gen_styp()
                .set_styp()
                .normalize_dts()
                .set_segno(sequence)
                .write(&mut legacy_media_file)
                .unwrap();
            sequence += 1;
        }
        parsed
            .normalize_and_dump(&mut File::create(&legacy_normalized).unwrap())
            .unwrap();

        assert_eq!(
            patch_init(&input, &media_output, &normalized_output, 19).unwrap(),
            sequence
        );
        assert_eq!(
            fs::read(media_output).unwrap(),
            fs::read(legacy_media).unwrap()
        );
        assert_eq!(
            fs::read(normalized_output).unwrap(),
            fs::read(legacy_normalized).unwrap()
        );
    }

    #[test]
    fn version_one_sidx_to_version_zero_tfdt_overflow_publishes_nothing() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("init.mp4");
        let media_output = temp.path().join("media.m4s");
        let normalized_output = temp.path().join("normalized.mp4");
        let fragment_moof = moof(1, Some((0, 3)));
        let fragment_mdat = mdat(b"payload");
        let raw_sidx = sidx(
            1,
            u64::from(u32::MAX) + 1,
            u32::try_from(fragment_moof.len() + fragment_mdat.len()).unwrap(),
        );
        let mut bytes = brand_box(FTYP);
        bytes.extend_from_slice(&standard_box(MOOV, &[]));
        bytes.extend_from_slice(&raw_sidx);
        bytes.extend_from_slice(&fragment_moof);
        bytes.extend_from_slice(&fragment_mdat);
        write(&input, &bytes);

        assert!(patch_init(&input, &media_output, &normalized_output, 0).is_err());
        assert!(!media_output.exists());
        assert!(!normalized_output.exists());
    }

    #[test]
    fn malformed_header_corpus_is_rejected_without_publication() {
        let mut overflow = standard_box(*b"free", &[]);
        overflow.extend_from_slice(&1_u32.to_be_bytes());
        overflow.extend_from_slice(&MDAT);
        overflow.extend_from_slice(&u64::MAX.to_be_bytes());
        let corpus = vec![
            ("short", vec![0, 0, 0, 8, b'm', b'd', b'a']),
            (
                "undersized",
                [4_u32.to_be_bytes().as_slice(), b"free"].concat(),
            ),
            (
                "short-extended",
                [1_u32.to_be_bytes().as_slice(), b"free", &[0, 0, 0, 16]].concat(),
            ),
            (
                "undersized-extended",
                [
                    1_u32.to_be_bytes().as_slice(),
                    b"free",
                    15_u64.to_be_bytes().as_slice(),
                ]
                .concat(),
            ),
            (
                "declared-past-end",
                [32_u32.to_be_bytes().as_slice(), b"free"].concat(),
            ),
            ("offset-overflow", overflow),
            (
                "trailing-header",
                [standard_box(*b"free", &[]), vec![0, 0, 0]].concat(),
            ),
            ("invalid-styp-width", standard_box(STYP, &[0; 7])),
        ];

        for (name, bytes) in corpus {
            let temp = tempfile::tempdir().unwrap();
            let input = temp.path().join(format!("{name}.m4s"));
            let output = temp.path().join("published.m4s");
            write(&input, &bytes);
            assert!(
                patch_media(&input, &output, 0).is_err(),
                "accepted {}",
                name
            );
            assert!(!output.exists(), "published {}", name);
        }
    }

    #[test]
    fn truncation_inside_moof_and_invalid_version_widths_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("published.m4s");

        let mut truncated_moof_content = 32_u32.to_be_bytes().to_vec();
        truncated_moof_content.extend_from_slice(&MFHD);
        let mut truncated = brand_box(STYP);
        truncated.extend_from_slice(&standard_box(MOOF, &truncated_moof_content));
        truncated.extend_from_slice(&mdat(b"payload"));
        let input = temp.path().join("truncated.m4s");
        write(&input, &truncated);
        assert!(patch_media(&input, &output, 0).is_err());
        assert!(!output.exists());

        let fragment_moof = moof(1, None);
        let fragment_mdat = mdat(b"payload");
        let mut invalid_sidx = sidx(
            1,
            1,
            u32::try_from(fragment_moof.len() + fragment_mdat.len()).unwrap(),
        );
        invalid_sidx[8] = 2;
        let mut invalid_version = brand_box(STYP);
        invalid_version.extend_from_slice(&invalid_sidx);
        invalid_version.extend_from_slice(&fragment_moof);
        invalid_version.extend_from_slice(&fragment_mdat);
        let input = temp.path().join("invalid-version.m4s");
        write(&input, &invalid_version);
        assert!(patch_media(&input, &output, 0).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn zero_sized_nonfinal_box_and_sequence_overflow_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("published.m4s");
        let mut zero_moof = brand_box(STYP);
        zero_moof.extend_from_slice(&zero_box(MOOF, &mfhd(1)));
        zero_moof.extend_from_slice(&mdat(b"unreachable"));
        let input = temp.path().join("zero-moof.m4s");
        write(&input, &zero_moof);
        assert!(patch_media(&input, &output, 0).is_err());
        assert!(!output.exists());

        write(&input, &media_fragment(1, b"payload"));
        assert!(patch_media(&input, &output, u32::MAX).is_err());
        assert!(!output.exists());
    }

    #[test]
    fn explicit_box_count_and_depth_limits_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("too-many.m4s");
        let output = temp.path().join("published.m4s");
        let mut boxes = Vec::with_capacity((MAX_BOX_COUNT + 1) * 8);
        for _ in 0..=MAX_BOX_COUNT {
            boxes.extend_from_slice(&standard_box(*b"free", &[]));
        }
        write(&input, &boxes);
        assert!(patch_media(&input, &output, 0).is_err());
        assert!(!output.exists());

        let mut file = File::open(&input).unwrap();
        let mut budget = ParseBudget::default();
        assert!(read_box_header(
            &mut file,
            fs::metadata(&input).unwrap().len(),
            MAX_BOX_DEPTH + 1,
            &mut budget
        )
        .is_err());
    }

    #[test]
    fn explicit_fragment_and_control_box_size_limits_are_enforced() {
        assert!(validate_fragment_size(0).is_err());
        assert!(validate_fragment_size(MAX_FRAGMENT_BYTES).is_ok());
        assert!(validate_fragment_size(MAX_FRAGMENT_BYTES + 1).is_err());

        let oversized_control = BoxSpan {
            start: 0,
            end: MAX_CONTROL_BOX_BYTES + 1,
            header_bytes: 8,
            kind: MOOF,
        };
        assert!(validate_control_box(oversized_control, "moof").is_err());
        let oversized_brand = BoxSpan {
            start: 0,
            end: MAX_BRAND_BOX_BYTES + 1,
            header_bytes: 8,
            kind: STYP,
        };
        let mut temp = tempfile::tempfile().unwrap();
        assert!(validate_brand_box(&mut temp, oversized_brand).is_err());
    }

    #[test]
    fn partial_styp_remains_an_incremental_init_signal() {
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("partial.m4s");
        let output = temp.path().join("published.m4s");
        write(&input, &brand_box(STYP));

        assert!(matches!(
            patch_media(&input, &output, 0),
            Err(NightfallError::PartialSegment(_))
        ));
        assert!(!output.exists());
    }

    #[test]
    fn large_sparse_mdat_is_copied_with_a_fixed_memory_window() {
        const PAYLOAD_BYTES: u64 = 16 * 1024 * 1024;
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("large.m4s");
        let output = temp.path().join("published.m4s");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&input)
            .unwrap();
        let prefix = [brand_box(STYP), moof(1, None)].concat();
        file.write_all(&prefix).unwrap();
        file.write_all(&u32::try_from(PAYLOAD_BYTES + 8).unwrap().to_be_bytes())
            .unwrap();
        file.write_all(&MDAT).unwrap();
        let payload_start = file.stream_position().unwrap();
        file.set_len(payload_start + PAYLOAD_BYTES).unwrap();
        drop(file);

        assert_eq!(patch_media(&input, &output, 5).unwrap(), 6);
        assert_eq!(
            fs::metadata(output).unwrap().len(),
            u64::try_from(prefix.len()).unwrap() + 8 + PAYLOAD_BYTES
        );
        assert_eq!(COPY_BUFFER_BYTES, 1024 * 1024);
        assert!(std::mem::size_of::<SegmentPlan>() < 256);
    }
}
