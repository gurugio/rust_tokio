use anyhow::{Context, Result};
use boolinator::Boolinator;
use futures::stream::StreamExt;
use futures::{future, Stream};
use inotify::{Event, EventMask, Inotify, WatchMask};
use std::path::PathBuf;
use tokio::runtime;

fn print_type_of<T>(_: &T) {
    println!("{}", std::any::type_name::<T>())
}

fn main() {
    // txtInotifyWatcher::new() returns future instance because it is "async fn".
    let iwatcher = TxtInotifyWatcher::new();
    print_type_of(&iwatcher);

    // create a runtime of tokio
    // that runs the async function.
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // This does not finish but loop on iwatcher
    let _ = rt.block_on(iwatcher);
}

pub struct TxtInotifyWatcher {}

impl TxtInotifyWatcher {
    async fn new() -> Result<()> {
        let buf: Vec<u8> = vec![0; 64];
        let path: PathBuf = std::env::current_dir()?.into();
        let watcher = Box::pin(InotifyQmpStream::new(buf, path.to_owned()).unwrap());

        let existing_sockets = FsQmpStream::new(path.to_owned()).unwrap();
        let mut qmp_sockets = existing_sockets.chain(watcher);
        while let Some(qmp) = qmp_sockets.next().await {
            println!("{:?}", qmp);
        }
        Ok(())
    }
}

struct InotifyQmpStream {}

impl InotifyQmpStream {
    async fn socket_path((ev, dir_path): (Event<std::ffi::OsString>, PathBuf)) -> Option<PathBuf> {
        match ev.mask {
            EventMask::CREATE => {
                let path = dir_path.join(ev.name?);
                let is_socket = path.extension()? == "txt";
                is_socket.as_some(path)
            }
            _ => None,
        }
    }
    pub fn new<T>(buffer: T, qmp_socket_path: PathBuf) -> Result<impl Stream<Item = PathBuf>>
    where
        T: AsMut<[u8]> + AsRef<[u8]>,
    {
        let mut inotify = Inotify::init()?;
        inotify
            .add_watch(&qmp_socket_path, WatchMask::CREATE)
            .with_context(|| {
                format!(
                    "Failed to open qmp socket directory directory {}\nCheck the `qmp_socket_directory` configuration variable",
                    qmp_socket_path.display()
                )
            })?;

        let inotify_stream = inotify
            .event_stream(buffer)?
            .take_while(|ev| future::ready(ev.is_ok()))
            .map(move |ev| (ev.unwrap(), qmp_socket_path.clone()))
            .filter_map(InotifyQmpStream::socket_path);

        Ok(inotify_stream)
    }
}

struct FsQmpStream {}

impl FsQmpStream {
    fn socket_path(dir_entry: std::fs::DirEntry) -> Option<PathBuf> {
        let file_type = dir_entry.file_type().ok()?;
        let is_socket = file_type.is_file() && !file_type.is_dir();
        is_socket.as_some(dir_entry.path())
    }

    pub fn new(qmp_socket_path: PathBuf) -> Result<impl Stream<Item = PathBuf>> {
        let fs_qmp_stream = futures::stream::iter(
            std::fs::read_dir(qmp_socket_path)?
                .flatten()
                .filter_map(FsQmpStream::socket_path),
        );
        Ok(fs_qmp_stream)
    }
}
