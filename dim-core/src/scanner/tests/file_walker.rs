use super::temp_dir;
use std::path::PathBuf;

fn synthetic_file(index: usize) -> super::super::DiscoveredFile {
    super::super::DiscoveredFile {
        path: PathBuf::from(format!("Movie {index}.mkv")),
        supported: true,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_walkdir() {
    let tempdir = temp_dir(vec![
        "file1.mkv",
        "file2.avi",
        "file3.txt",
        "a/b/file4.webm",
        "a/file5.mp4",
        ".hidden.mp4",
    ]);

    let mut files = super::super::get_subfiles([tempdir.path()].iter());
    files.sort();

    let mut expected: Vec<PathBuf> =
        IntoIterator::into_iter(["file1.mkv", "file2.avi", "a/b/file4.webm", "a/file5.mp4"])
            .map(|x| tempdir.path().join(x))
            .collect();

    expected.sort();

    assert_eq!(files, expected);
}

#[test]
fn mixed_discovery_keeps_unsupported_results_classifiable() {
    let directory = super::temp_dir(vec!["Movie.mkv", "notes.txt"]);
    let discovered = super::super::discover_files(std::iter::once(directory.path()));
    assert_eq!(discovered.len(), 2);
    assert_eq!(discovered.iter().filter(|file| file.supported).count(), 1);
    assert_eq!(discovered.iter().filter(|file| !file.supported).count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn large_discovery_is_backpressured_instead_of_collected() {
    const FILES: usize = 10_000;
    let (mut receiver, worker) = super::super::spawn_discovery_worker(|emit| {
        let mut emitted = 0;
        for index in 0..FILES {
            if !emit(synthetic_file(index)) {
                break;
            }
            emitted += 1;
        }
        Ok(emitted)
    });

    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while receiver.len() < super::super::DISCOVERY_QUEUE_CAPACITY {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("discovery producer never filled its bounded queue");
    assert_eq!(receiver.len(), super::super::DISCOVERY_QUEUE_CAPACITY);
    assert!(
        !worker.is_finished(),
        "producer should block rather than collect the remaining filesystem tree"
    );

    let mut received = 0;
    while receiver.recv().await.is_some() {
        received += 1;
    }
    let stats = worker.await.unwrap().unwrap();
    assert_eq!(received, FILES);
    assert_eq!(stats.files, FILES);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_discovery_consumer_cancels_a_backpressured_producer() {
    let (receiver, worker) = super::super::spawn_discovery_worker(|emit| {
        let mut emitted = 0;
        for index in 0..10_000 {
            if !emit(synthetic_file(index)) {
                break;
            }
            emitted += 1;
        }
        Ok(emitted)
    });

    drop(receiver);
    let stats = tokio::time::timeout(std::time::Duration::from_secs(2), worker)
        .await
        .expect("cancelled discovery producer remained blocked")
        .unwrap()
        .unwrap();
    assert!(stats.files < 10_000);
}

#[tokio::test(flavor = "multi_thread")]
async fn traversal_failure_after_streamed_files_is_propagated() {
    let (mut receiver, worker) = super::super::spawn_discovery_worker(|emit| {
        for index in 0..32 {
            assert!(emit(synthetic_file(index)));
        }
        Err(super::super::Error::FilesystemTraversal {
            path: "synthetic-root".into(),
            message: "directory became unreadable".into(),
        })
    });

    let mut received = 0;
    while receiver.recv().await.is_some() {
        received += 1;
    }
    assert_eq!(received, 32);
    let error = worker.await.unwrap().unwrap_err();
    assert!(error.to_string().contains("directory became unreadable"));
}
