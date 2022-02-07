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
    // get a future instance because it is "async fn".
    let iwatcher = newwatcher();
    print_type_of(&iwatcher); // check the exact type name

    // create a runtime of tokio
    // that runs the async function.
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // This does not finish but loop on iwatcher
    let _ = rt.block_on(iwatcher);
}

async fn newwatcher() -> Result<()> {
    // just an internal buffer for Inotify stream
    let buf: Vec<u8> = vec![0; 64];
    // PathBuf instance for the current directory
    let path: PathBuf = std::env::current_dir()?.into();
    // Box::pin is mandatory for .next() method
    // because .next() requires unpin.
    let new_files = Box::pin(InotifyTxtStream::new(buf, path.to_owned()).unwrap());

    // add inotify for existing txt files
    let existing_files = FsTxtStream::new(path.to_owned()).unwrap();
    let mut txt_files = existing_files.chain(new_files);
    while let Some(name) = txt_files.next().await {
        println!("{:?}", name);
    }
    Ok(())
}

struct InotifyTxtStream {}

impl InotifyTxtStream {
    async fn txt_path((ev, dir_path): (Event<std::ffi::OsString>, PathBuf)) -> Option<PathBuf> {
        match ev.mask {
            EventMask::CREATE => {
                let path = dir_path.join(ev.name?);
                let is_txt = path.extension()? == "txt";
                is_txt.as_some(path)
            }
            _ => None,
        }
    }
    /// return an inotify instance streaming target path
    pub fn new(buffer: Vec<u8>, target_path: PathBuf) -> Result<impl Stream<Item = PathBuf>> {
        // make a inotify instance watching create event
        let mut inotify = Inotify::init()?;
        inotify
            .add_watch(&target_path, WatchMask::CREATE)
            .with_context(|| {
                format!(
                    "Failed to open target directory {}\n",
                    target_path.display()
                )
            })?;

        // event-stream: multi-event
        // take_while: take event only ready -> why?
        // map: create a pair (event, path)
        // filter_map: filtering only path has txt extension
        let inotify_stream = inotify
            .event_stream(buffer)?
            .take_while(|ev| future::ready(ev.is_ok()))
            .map(move |ev| (ev.unwrap(), target_path.clone()))
            .filter_map(InotifyTxtStream::txt_path);

        // return a stream that return PathBuf including txt
        Ok(inotify_stream)
    }
}

struct FsTxtStream {}

impl FsTxtStream {
    fn txt_path(dir_entry: std::fs::DirEntry) -> Option<PathBuf> {
        let file_type = dir_entry.file_type().ok()?;
        let is_txt =
            file_type.is_file() && !file_type.is_dir() && dir_entry.path().extension()? == "txt";
        is_txt.as_some(dir_entry.path())
    }

    pub fn new(target_path: PathBuf) -> Result<impl Stream<Item = PathBuf>> {
        // futures::stream::iter: convert iterator to stream
        // read_dir: iterator to read target path
        let fs_txt_stream = futures::stream::iter(
            std::fs::read_dir(target_path)?
                .flatten()
                .filter_map(FsTxtStream::txt_path),
        );
        // return a stream that return PathBuf including txt
        Ok(fs_txt_stream)
    }
}
