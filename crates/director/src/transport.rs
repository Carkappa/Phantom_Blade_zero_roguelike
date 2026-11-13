//! How command lines and state text travel between the director and the bridge.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub trait Transport {
    /// Puts one encoded command line where the bridge reads it, replacing any earlier one.
    fn send(&mut self, line: &str) -> io::Result<()>;
    /// Empties the inbox once a command is answered.
    fn clear(&mut self) -> io::Result<()>;
    /// The bridge's latest state text, if there is any. It may be half-written; the parser decides.
    fn read_state(&mut self) -> Option<String>;
}

/// The real thing: `inbox.tsv` and `state.tsv` in the mod's `Runtime` folder.
pub struct FileTransport {
    inbox: PathBuf,
    scratch: PathBuf,
    state: PathBuf,
}

impl FileTransport {
    pub fn new(dir: &Path) -> io::Result<FileTransport> {
        fs::create_dir_all(dir)?;
        let transport = FileTransport {
            inbox: dir.join("inbox.tsv"),
            scratch: dir.join("inbox.tmp"),
            state: dir.join("state.tsv"),
        };
        // A command left by an earlier director must not run under this one.
        fs::write(&transport.inbox, "")?;
        Ok(transport)
    }

    fn put(&self, content: &str) -> io::Result<()> {
        // Write beside the inbox and rename over it, so the bridge never sees half a line. On
        // Windows the rename is refused while the bridge has the inbox open; then write in place:
        // the bridge ignores a line without its final newline and looks again.
        fs::write(&self.scratch, content)?;
        match fs::rename(&self.scratch, &self.inbox) {
            Ok(()) => Ok(()),
            Err(_) => {
                let result = fs::write(&self.inbox, content);
                let _ = fs::remove_file(&self.scratch);
                result
            }
        }
    }
}

impl Transport for FileTransport {
    fn send(&mut self, line: &str) -> io::Result<()> {
        self.put(line)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.put("")
    }

    fn read_state(&mut self) -> Option<String> {
        let bytes = fs::read(&self.state).ok()?;
        String::from_utf8(bytes).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pbzr-transport-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn commands_replace_each_other_and_clear_empties_the_inbox() {
        let dir = temp_dir("send");
        let mut t = FileTransport::new(&dir.join("deep").join("Runtime")).unwrap();
        let inbox = dir.join("deep").join("Runtime").join("inbox.tsv");
        assert_eq!(
            fs::read_to_string(&inbox).unwrap(),
            "",
            "a new director starts with an empty inbox"
        );
        t.send("PBZR1\ta\tclear\n").unwrap();
        assert_eq!(fs::read_to_string(&inbox).unwrap(), "PBZR1\ta\tclear\n");
        t.send("PBZR1\tb\theal\t5\n").unwrap();
        assert_eq!(fs::read_to_string(&inbox).unwrap(), "PBZR1\tb\theal\t5\n");
        t.clear().unwrap();
        assert_eq!(fs::read_to_string(&inbox).unwrap(), "");
        assert!(
            !dir.join("deep").join("Runtime").join("inbox.tmp").exists(),
            "no scratch file is left behind"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_command_is_wiped_at_start() {
        let dir = temp_dir("stale");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("inbox.tsv"),
            "PBZR1\told\tspawn\tu1\tBP_X\t5\t100\t0\t1\n",
        )
        .unwrap();
        let _t = FileTransport::new(&dir).unwrap();
        assert_eq!(fs::read_to_string(dir.join("inbox.tsv")).unwrap(), "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn state_is_read_when_present() {
        let dir = temp_dir("state");
        let mut t = FileTransport::new(&dir).unwrap();
        assert_eq!(t.read_state(), None);
        fs::write(dir.join("state.tsv"), "begin\t1\n").unwrap();
        assert_eq!(t.read_state().as_deref(), Some("begin\t1\n"));
        // Bytes cut in the middle of a character are not a state.
        fs::write(dir.join("state.tsv"), [0xE4u8, 0xB8]).unwrap();
        assert_eq!(t.read_state(), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
