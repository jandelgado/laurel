use std::io::{Error,ErrorKind,Result,Seek,SeekFrom,Write};
use std::fs::{self,File,OpenOptions};
use std::ffi::{OsString,OsStr};
use posix_acl::{PosixACL, Qualifier, ACL_READ};
use std::os::unix::io::AsRawFd;

use nix::sys::stat::{fchmod,Mode};
use nix::unistd::Uid;

/// A rotating (log) file writer
///
/// [`FileRotate`] rotates the file after `filesize` bytes have been
/// written to the main file. Up to `num_files` generations of backup
/// files are kept around.
pub struct FileRotate {
    /// The name for the main file. For backup generations, `.1`,
    /// `.2`, `.3` etc. are appended to this file name.
    pub basename: OsString,
    /// When a [`write`] operation causes the main file to reach this
    /// size, a [`FileRotate::rotate`] operation is triggered.
    pub filesize: u64,
    pub generations: u64,
    pub uids: Vec<Uid>,
    file: Option<File>,
    offset: u64,
}

impl<'a> FileRotate {
    /// Creates a new [`FileRotate`] instance. This does not involve
    /// any I/O operations; the main file is only created when calling
    /// [`write`].
    pub fn new<P: AsRef<OsStr>>(path: P) -> Self {
        FileRotate {
            basename: OsString::from(path.as_ref()),
            filesize: 10 * 1024 * 1024,
            generations: 5,
            uids: vec!(),
            file: None,
            offset: 0,
        }
    }

    pub fn with_filesize(mut self, p: u64) -> Self { self.filesize = p; self }
    pub fn with_generations(mut self, p: u64) -> Self { self.generations = p; self }
    pub fn with_uid(mut self, uid: Uid) -> Self { self.uids.push(uid); self }

    /// Closes the main file and performs a backup file rotation
    pub fn rotate(&mut self) -> Result<()> {
        for suffix in (0..self.generations).rev() {
            let mut old = self.basename.clone();
            match suffix {
                0 => (),
                _ => old.push(format!(".{}", suffix)),
            };
            let mut new = self.basename.clone();
            new.push(format!(".{}", suffix + 1));
            if let Ok(_) = fs::metadata(&old) {
                fs::rename(old, new)?;
            }
        }
        self.file = None;
        Ok(())
    }

    fn open(&mut self) -> Result<()> {
        let mut file = OpenOptions::new().create(true).append(true).open(&self.basename)?;
        fchmod(file.as_raw_fd(), Mode::from_bits(0o600).unwrap())
            .map_err(|e|Error::new(ErrorKind::Other, e))?;
        let mut acl = PosixACL::new(0o600);
        for uid in &self.uids {
            acl.set(Qualifier::User(uid.as_raw()), ACL_READ);
        }
        acl.write_acl(&self.basename).map_err(|e|Error::new(e.kind(), e))?;
        self.offset = file.seek(SeekFrom::End(0))?;
        self.file = Some(file);
        Ok(())
    }
}

impl Write for FileRotate {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        if let None = self.file {
            self.open()?;
        }
        let mut f = self.file.as_ref().unwrap();
        let sz = f.write(buf)?;
        self.offset += sz as u64;
        if self.offset > self.filesize {
            f.sync_all()?;
            self.rotate()?;
        }
        Ok(sz)
    }
    fn flush(&mut self) -> Result<()> {
        match self.file.as_ref() {
            Some(mut f) => f.flush(),
            None => Ok(()),
        }
    }
}
