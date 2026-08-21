pub use anitomy::Anitomy;
use anitomy::ElementCategory;
pub use torrent_name_parser::Metadata as TorrentMetadata;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Metadata {
    pub name: String,
    pub year: Option<i64>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
}

pub trait FilenameMetadata {
    fn from_str(s: &str) -> Option<Metadata>;
}

/// Remove distribution-only prefixes before handing a release name to the specialised parsers.
/// Keep this deliberately anchored: bracketed text and numbers elsewhere can be legitimate title
/// content (for example `The [REC] Collection` or `2001 A Space Odyssey`).
fn clean_release_name(input: &str) -> &str {
    let mut value = input.trim();

    loop {
        if !value.starts_with('[') {
            break;
        }
        let Some(end) = value.find(']') else {
            break;
        };
        let tag = &value[1..end];
        let source_like = tag.contains(['.', '/', '@'])
            || tag.to_ascii_lowercase().starts_with("www")
            || tag.to_ascii_lowercase().contains("torrent");
        if !source_like {
            break;
        }
        let remainder = value[end + 1..].trim_start_matches([' ', '.', '_', '-']);
        if remainder.is_empty() {
            break;
        }
        value = remainder;
    }

    let digits = value.bytes().take_while(u8::is_ascii_digit).count();
    if digits >= 2 && digits <= 4 && value.starts_with('0') {
        let remainder = &value[digits..];
        if remainder.starts_with([' ', '.', '_', '-']) {
            let remainder = remainder.trim_start_matches([' ', '.', '_', '-']);
            if remainder.chars().any(char::is_alphabetic) {
                value = remainder;
            }
        }
    }

    value
}

impl FilenameMetadata for TorrentMetadata {
    fn from_str(s: &str) -> Option<Metadata> {
        let metadata = TorrentMetadata::from(clean_release_name(s)).ok()?;

        Some(Metadata {
            name: metadata.title().to_owned(),
            year: metadata.year().map(|x| x as i64),
            season: metadata.season().map(|x| x as i64),
            episode: metadata.episode().map(|x| x as i64),
        })
    }
}

impl FilenameMetadata for Anitomy {
    fn from_str(s: &str) -> Option<Metadata> {
        let metadata = match Anitomy::new().parse(clean_release_name(s)) {
            Ok(v) | Err(v) => v,
        };

        Some(Metadata {
            name: metadata.get(ElementCategory::AnimeTitle)?.to_string(),
            year: metadata
                .get(ElementCategory::AnimeYear)
                .and_then(|x| x.parse().ok()),
            // If season isnt specified we assume season 1 here.
            season: metadata
                .get(ElementCategory::AnimeSeason)
                .and_then(|x| x.parse().ok())
                .or(Some(1)),
            episode: metadata
                .get(ElementCategory::EpisodeNumber)
                .and_then(|x| x.parse().ok()),
        })
    }
}

/// A special filename metadata extractor that combines torrent_name_parser and anitomy which in
/// some cases is necessary. TNP is really good at extracting show titles but not season and
/// episode numbers. Anitomy excels at this. Here we combine the title extracted by TPN and the
/// season and episode number extracted by Anitomy.
pub struct CombinedExtractor;

impl FilenameMetadata for CombinedExtractor {
    fn from_str(s: &str) -> Option<Metadata> {
        let cleaned = clean_release_name(s);
        let metadata_tnp = TorrentMetadata::from(cleaned).ok()?;
        let metadata_anitomy = match Anitomy::new().parse(cleaned) {
            Ok(v) | Err(v) => v,
        };

        Some(Metadata {
            name: metadata_tnp.title().to_owned(),
            year: metadata_tnp.year().map(|x| x as i64),
            // If season isnt specified we assume season 1 here as some releases only have a
            // episode number and no season number.
            season: metadata_anitomy
                .get(ElementCategory::AnimeSeason)
                .and_then(|x| x.parse().ok())
                .or(Some(1)),
            episode: metadata_anitomy
                .get(ElementCategory::EpisodeNumber)
                .and_then(|x| x.parse().ok()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_release_name, FilenameMetadata, TorrentMetadata};

    #[test]
    fn strips_source_tag_and_zero_padded_index_before_release_parsing() {
        let parsed = TorrentMetadata::from_str(
            "[scloudx.lol] 021.City.of.God.2002.BluRay.1080p.x265.10bit.MNHD-FRDS",
        )
        .unwrap();

        assert_eq!(parsed.name, "City of God");
        assert_eq!(parsed.year, Some(2002));
    }

    #[test]
    fn preserves_legitimate_numeric_and_bracketed_title_content() {
        let numeric = TorrentMetadata::from_str("2001.A.Space.Odyssey.1968.1080p.BluRay").unwrap();
        assert_eq!(numeric.name, "2001 A Space Odyssey");
        assert_eq!(numeric.year, Some(1968));

        let bracketed = TorrentMetadata::from_str("The.[REC].Collection.2007.1080p").unwrap();
        assert!(bracketed.name.contains("REC"));

        assert_eq!(
            clean_release_name("[REC].2007.1080p.BluRay"),
            "[REC].2007.1080p.BluRay"
        );
    }
}
