use mp4::mp4box::{BoxHeader, BoxType, FtypBox, MoofBox, WriteBox};
use std::collections::VecDeque;
use std::env;
use std::fs::{self, File};
use std::io::{self, BufReader, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

static ZERO_CHUNK: [u8; 64 * 1024] = [0; 64 * 1024];

#[derive(Clone, Copy)]
enum Kind {
    Init,
    LegacyInit,
    LegacyMedia,
    Media,
}

impl Kind {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "init" => Ok(Self::Init),
            "legacy-init" => Ok(Self::LegacyInit),
            "legacy-media" => Ok(Self::LegacyMedia),
            "media" => Ok(Self::Media),
            _ => Err(format!(
                "unknown benchmark kind {value:?}; expected init, legacy-init, legacy-media, or media"
            )),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::LegacyInit => "legacy-init",
            Self::LegacyMedia => "legacy-media",
            Self::Media => "media",
        }
    }
}

fn write_payload(file: &mut File, mut remaining: u64) -> io::Result<()> {
    while remaining != 0 {
        let count = remaining.min(ZERO_CHUNK.len() as u64) as usize;
        file.write_all(&ZERO_CHUNK[..count])?;
        remaining -= count as u64;
    }
    Ok(())
}

fn write_fragment(
    path: &Path,
    kind: Kind,
    payload_bytes: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = File::create(path)?;
    if matches!(kind, Kind::Init | Kind::LegacyInit) {
        FtypBox {
            box_type: BoxType::FtypBox,
            ..Default::default()
        }
        .write_box(&mut file)?;
        BoxHeader {
            name: BoxType::MoovBox,
            size: 8,
        }
        .write(&mut file)?;
    }

    MoofBox::default().write_box(&mut file)?;
    let mdat_size = payload_bytes.checked_add(8).ok_or("mdat size overflow")?;
    if mdat_size > u32::MAX.into() {
        return Err("benchmark payload currently requires a 32-bit mdat size".into());
    }
    BoxHeader {
        name: BoxType::MdatBox,
        size: mdat_size,
    }
    .write(&mut file)?;
    write_payload(&mut file, payload_bytes)?;
    file.sync_all()?;
    Ok(())
}

fn legacy_write_new(
    destination: &Path,
    write: impl FnOnce(&mut File) -> Result<(), Box<dyn std::error::Error>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = destination.with_extension("legacy-replacement");
    let mut file = File::create(&temporary)?;
    if let Err(error) = write(&mut file) {
        let _ = fs::remove_file(temporary);
        return Err(error);
    }
    file.flush()?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, destination)?;
    Ok(())
}

fn legacy_patch_init(
    input: &Path,
    output: &Path,
    normalized: &Path,
    mut sequence: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(input)?;
    let size = file.metadata()?.len();
    let mut init =
        nightfall::patch::init_segment::InitSegment::from_reader(BufReader::new(file), size)?;
    let mut embedded = std::mem::take(&mut init.segments);
    if embedded.is_empty()
        || embedded
            .iter()
            .any(|segment| segment.moof.is_none() || segment.mdat.is_none())
    {
        return Err("legacy parser did not find complete embedded media".into());
    }

    legacy_write_new(normalized, |file| {
        init.normalize_and_dump(file)?;
        Ok(())
    })?;
    legacy_write_new(output, |file| {
        while let Some(segment) = embedded.pop_front() {
            segment
                .gen_styp()
                .set_styp()
                .normalize_dts()
                .set_segno(sequence)
                .write(file)?;
            sequence = sequence.checked_add(1).ok_or("legacy sequence overflow")?;
        }
        Ok(())
    })?;
    Ok(())
}

fn legacy_patch_media(
    input: &Path,
    output: &Path,
    mut sequence: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(input)?;
    let size = file.metadata()?.len();
    let mut reader = BufReader::new(file);
    let mut segments = VecDeque::new();
    let mut current = reader.stream_position()?;
    while current < size {
        let (segment, new_position) =
            nightfall::patch::segment::Segment::from_reader(&mut reader, size)?;
        segments.push_back(segment);
        current = new_position;
    }
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| segment.moof.is_none() || segment.mdat.is_none())
    {
        return Err("legacy parser did not find complete media".into());
    }

    legacy_write_new(output, |file| {
        while let Some(segment) = segments.pop_front() {
            segment.gen_styp().set_segno(sequence).write(file)?;
            sequence = sequence.checked_add(1).ok_or("legacy sequence overflow")?;
        }
        Ok(())
    })
}

async fn run_once(
    root: &Path,
    kind: Kind,
    payload_bytes: u64,
    iteration: usize,
) -> Result<Duration, Box<dyn std::error::Error>> {
    let input = root.join(format!("{}-{iteration}.input", kind.label()));
    let output = root.join(format!("{}-{iteration}.output", kind.label()));
    let normalized = root.join(format!("{}-{iteration}.normalized", kind.label()));
    write_fragment(&input, kind, payload_bytes)?;

    let started = Instant::now();
    match kind {
        Kind::Media => {
            nightfall::patch::segment::patch_segment_to(input.clone(), output.clone(), 7).await?;
        }
        Kind::Init => {
            nightfall::patch::init_segment::patch_init_segment_to(
                input.clone(),
                output.clone(),
                normalized.clone(),
                7,
            )
            .await?;
        }
        Kind::LegacyInit => legacy_patch_init(&input, &output, &normalized, 7)?,
        Kind::LegacyMedia => legacy_patch_media(&input, &output, 7)?,
    }
    let elapsed = started.elapsed();

    fs::remove_file(input)?;
    fs::remove_file(output)?;
    if normalized.exists() {
        fs::remove_file(normalized)?;
    }
    Ok(elapsed)
}

fn usage(binary: &str) -> String {
    format!(
        "usage: {binary} <init|legacy-init|legacy-media|media> <payload-bytes> <iterations> [working-directory]"
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 || args.len() > 5 {
        return Err(usage(&args[0]).into());
    }
    let kind = Kind::parse(&args[1])?;
    let payload_bytes: u64 = args[2].parse()?;
    let iterations: usize = args[3].parse()?;
    if iterations == 0 {
        return Err("iterations must be nonzero".into());
    }

    let parent = args.get(4).map(PathBuf::from).unwrap_or_else(env::temp_dir);
    let root = parent.join(format!(
        "nightfall-patch-benchmark-{}-{}",
        std::process::id(),
        kind.label()
    ));
    fs::create_dir_all(&root)?;

    let result = async {
        let mut total = Duration::default();
        for iteration in 0..iterations {
            total += run_once(&root, kind, payload_bytes, iteration).await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(total)
    }
    .await;
    let _ = fs::remove_dir_all(&root);
    let total = result?;
    println!(
        "kind={} payload_bytes={} iterations={} total_patch_ms={:.3} mean_patch_ms={:.3}",
        kind.label(),
        payload_bytes,
        iterations,
        total.as_secs_f64() * 1_000.0,
        total.as_secs_f64() * 1_000.0 / iterations as f64
    );
    Ok(())
}
