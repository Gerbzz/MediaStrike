use std::path::{Path, PathBuf};

pub const MAX_QUEUE_ITEMS: usize = 1_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat all",
            Self::One => "Repeat one",
        }
    }
}

#[derive(Default)]
pub struct MediaQueue {
    items: Vec<PathBuf>,
    current: Option<usize>,
    repeat: RepeatMode,
}

impl MediaQueue {
    pub fn items(&self) -> &[PathBuf] {
        &self.items
    }

    pub fn current(&self) -> Option<usize> {
        self.current
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
    }

    /// Returns the first newly enqueued index. Duplicate paths remain separate playlist entries.
    pub fn enqueue(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> Option<usize> {
        let first = self.items.len();
        self.items
            .extend(paths.into_iter().take(MAX_QUEUE_ITEMS - first));
        (self.items.len() > first).then_some(first)
    }

    pub fn path(&self, index: usize) -> Option<&Path> {
        self.items.get(index).map(PathBuf::as_path)
    }

    /// Commit selection only after the player successfully opens the target.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.items.len() {
            return false;
        }
        self.current = Some(index);
        true
    }

    pub fn next_index(&self) -> Option<usize> {
        match self.current {
            None if !self.items.is_empty() => Some(0),
            Some(index) if index + 1 < self.items.len() => Some(index + 1),
            Some(_) if self.repeat == RepeatMode::All => Some(0),
            _ => None,
        }
    }

    pub fn previous_index(&self) -> Option<usize> {
        match self.current {
            Some(index) if index > 0 => Some(index - 1),
            Some(0) if self.repeat == RepeatMode::All => Some(self.items.len() - 1),
            _ => None,
        }
    }

    pub fn finished_index(&self) -> Option<usize> {
        if self.repeat == RepeatMode::One {
            self.current
        } else {
            self.next_index()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_QUEUE_ITEMS, MediaQueue, RepeatMode};
    use std::path::PathBuf;

    fn files(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn keeps_selection_until_a_new_file_is_opened() {
        let mut queue = MediaQueue::default();
        assert_eq!(queue.enqueue(files(&["a.mp4", "b.mp4"])), Some(0));
        assert_eq!(queue.current(), None);
        assert_eq!(queue.next_index(), Some(0));
        assert!(!queue.select(4));
        assert_eq!(queue.current(), None);
        assert!(queue.select(0));
        assert_eq!(queue.next_index(), Some(1));
        assert_eq!(queue.previous_index(), None);
        assert!(queue.select(1));
        assert_eq!(queue.next_index(), None);
        assert_eq!(queue.previous_index(), Some(0));
    }

    #[test]
    fn repeat_modes_distinguish_manual_skip_from_end_of_stream() {
        let mut queue = MediaQueue::default();
        queue.enqueue(files(&["a.mp4", "b.mp4"]));
        queue.select(1);
        assert_eq!(queue.finished_index(), None);
        queue.cycle_repeat();
        assert_eq!(queue.repeat(), RepeatMode::All);
        assert_eq!(queue.next_index(), Some(0));
        assert_eq!(queue.finished_index(), Some(0));
        queue.cycle_repeat();
        assert_eq!(queue.repeat(), RepeatMode::One);
        assert_eq!(queue.next_index(), None);
        assert_eq!(queue.finished_index(), Some(1));
        queue.cycle_repeat();
        assert_eq!(queue.repeat(), RepeatMode::Off);
    }

    #[test]
    fn bounds_the_session_queue() {
        let mut queue = MediaQueue::default();
        assert_eq!(
            queue.enqueue((0..=MAX_QUEUE_ITEMS).map(|i| i.to_string().into())),
            Some(0)
        );
        assert_eq!(queue.items().len(), MAX_QUEUE_ITEMS);
        assert_eq!(queue.enqueue(files(&["extra.mp4"])), None);
    }
}
