use notify::{
    self, watcher, DebouncedEvent, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::io::{BufReader, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::channel;
use std::time::Duration;
use walkdir::WalkDir;

type HandleMap = HashMap<String, Handle>;
type BoxError = Box<Error>;
type FileFilter = Rc<Fn(&str) -> bool>;
type LineFilter = Rc<Fn(&str) -> bool>;

#[derive(Debug)]
struct Handle {
    pos: u64,
    fd: File,
    path: PathBuf,
}

#[allow(dead_code)]
pub enum WatcherType {
    Poll,
    Recommend,
}

enum NotifyWatcherType {
    PollWatcher(PollWatcher),
    RecommendedWatcher(RecommendedWatcher),
}

impl NotifyWatcherType {
    fn watch(&mut self, dir: &str, mode: RecursiveMode) -> Result<(), notify::Error> {
        match self {
            NotifyWatcherType::PollWatcher(watcher) => watcher.watch(dir, mode),
            NotifyWatcherType::RecommendedWatcher(watcher) => watcher.watch(dir, mode),
        }
    }
}

pub struct WatchOption {
    dir: String,
    debounce_seconds: u64,
    watcher_type: WatcherType,
    // Determin if the file should be watched
    file_filter: FileFilter,
    // Determin if the line should be collected
    line_filter: LineFilter,
    // TODO:
    // Support Transform
}

fn identity(_: &str) -> bool {
    true
}

#[allow(dead_code)]
impl WatchOption {
    pub fn new(dir: String, seconds: u64) -> Self {
        WatchOption {
            dir,
            debounce_seconds: seconds,
            watcher_type: WatcherType::Recommend,
            file_filter: Rc::new(identity),
            line_filter: Rc::new(identity),
        }
    }

    pub fn watcher_type(mut self, watcher_type: WatcherType) -> Self {
        self.watcher_type = watcher_type;
        self
    }

    pub fn file_filter(mut self, filter: FileFilter) -> Self {
        self.file_filter = filter;
        self
    }

    pub fn line_filter(mut self, filter: LineFilter) -> Self {
        self.line_filter = filter;
        self
    }
}

pub fn watch_dir<F: ?Sized>(option: &WatchOption, callback: &F) -> Result<(), Box<Error>>
where
    F: Fn(&str, Vec<String>),
{
    let mut fds: HandleMap = register_dir(&option.dir, &option.file_filter)?;

    let (tx, rx) = channel();
    let mut watcher = match option.watcher_type {
        WatcherType::Poll => NotifyWatcherType::PollWatcher(PollWatcher::new(
            tx,
            Duration::from_secs(option.debounce_seconds),
        )?),
        WatcherType::Recommend => NotifyWatcherType::RecommendedWatcher(watcher(
            tx,
            Duration::from_secs(option.debounce_seconds),
        )?),
    };

    watcher.watch(&option.dir, RecursiveMode::Recursive)?;

    loop {
        match rx.recv() {
            Ok(event) => {
                match collect_logs(
                    event,
                    &mut fds,
                    option.file_filter.clone(),
                    option.line_filter.clone(),
                ) {
                    Ok(Some((name, logs))) => callback(&name, logs),
                    Err(e) => println!("watch error: {:?}", e),
                    _ => {}
                }
            }
            Err(e) => println!("watch error: {:?}", e),
        }
    }
}

fn register_dir(dir: &str, filter: &FileFilter) -> Result<(HandleMap), Box<Error>> {
    let mut fds: HandleMap = HashMap::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if let Some(name) = path.file_name() {
            if path.is_file() && filter(name.to_str().unwrap()) {
                insert_handle(&mut fds, path, true)?;
            }
        }
    }
    Ok(fds)
}

fn collect_logs(
    event: DebouncedEvent,
    fds: &mut HandleMap,
    file_filter: FileFilter,
    line_filter: LineFilter,
) -> Result<Option<(String, Vec<String>)>, BoxError> {
    // println!("Receive {:?}", event);
    match event {
        DebouncedEvent::Create(p) | DebouncedEvent::NoticeWrite(p) | DebouncedEvent::Write(p) => {
            return collect(fds, &p, file_filter, line_filter)
        }
        DebouncedEvent::NoticeRemove(p) | DebouncedEvent::Remove(p) => remove_handle(fds, &p)?,
        _ => {}
    }
    Ok(None)
}

fn insert_handle(fds: &mut HandleMap, path: &Path, at_tail: bool) -> Result<(), BoxError> {
    let name = path.file_name().unwrap().to_str().unwrap();
    let mut fd = File::open(path).unwrap();
    let meta = &fd.metadata().unwrap();

    let pos = if at_tail { meta.len() } else { 0 };
    fd.seek(SeekFrom::Start(pos))?;

    let handle = Handle {
        pos,
        fd,
        path: path.to_path_buf(),
    };
    println!("Resitered {}, pos: {}", name, pos);
    fds.insert(name.to_string(), handle);
    Ok(())
}

fn remove_handle(fds: &mut HandleMap, path: &Path) -> Result<(), BoxError> {
    let name = path.file_name().unwrap().to_str().unwrap();
    if fds.get(name).is_some() {
        fds.remove(name).unwrap();
    }
    println!("Handle removed, {}", name);
    Ok(())
}

fn collect(
    fds: &mut HandleMap,
    path: &Path,
    file_filter: FileFilter,
    line_filter: LineFilter,
) -> Result<Option<(String, Vec<String>)>, BoxError> {
    match path.file_name() {
        Some(file) => {
            match file.to_str() {
                Some(name) => {
                    if !file_filter(name) {
                        return Ok(None);
                    }
                    let mut handle = match fds.get_mut(name) {
                        Some(handle) => handle,
                        None => {
                            // When rotating
                            // Should trigger `Remove` and `Create`
                            // But we will not get any of them under debouncing mode
                            // So reopen file here
                            insert_handle(fds, path, false)?;
                            println!("File rotated, reopened: {}", name);
                            return collect(fds, path, file_filter, line_filter);
                        }
                    };
                    let meta = &handle.fd.metadata()?;
                    let end = meta.len();

                    let mut logs = Vec::new();
                    while handle.pos < end {
                        let mut reader = BufReader::new(&handle.fd);
                        let mut line = String::new();
                        let len = reader.read_line(&mut line)?;
                        if line_filter(&line) {
                            logs.push(line);
                        }
                        handle.pos += len as u64;
                        handle.fd.seek(SeekFrom::Start(handle.pos))?;
                    }

                    if end < handle.pos {
                        // Reset
                        handle.pos = 0;
                    }
                    Ok(Some((String::from(name), logs)))
                }
                None => Ok(None),
            }
        }
        None => Ok(None),
    }
}
