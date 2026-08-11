pub mod auth;
pub mod dashboard;
pub mod filebrowser;
pub mod library;
pub mod media;
pub mod mediafile;
pub mod search;
pub mod settings;
pub mod statik;
pub mod stream;
pub mod tv;
pub mod user;
pub mod websocket;

pub(crate) fn public_file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "media".into())
}

#[cfg(test)]
mod path_redaction_tests {
    #[test]
    fn absolute_media_paths_are_reduced_to_display_names() {
        assert_eq!(
            super::public_file_name("/srv/private/media/movie.mkv"),
            "movie.mkv"
        );
        assert!(!super::public_file_name("/srv/private/media/movie.mkv").contains("/srv"));
    }
}
