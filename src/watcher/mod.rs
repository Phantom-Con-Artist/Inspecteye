use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherEvent {
    Created(PathBuf),
    Modified(PathBuf),
    Deleted(PathBuf),
    Renamed { from: PathBuf, to: PathBuf },
}

pub trait ProjectWatcher: Send + Sync {
    fn start(&mut self) -> crate::error::Result<()>;
    fn stop(&mut self) -> crate::error::Result<()>;
}
