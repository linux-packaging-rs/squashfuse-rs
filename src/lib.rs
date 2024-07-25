use std::{
    ffi::OsStr,
    io::Read,
    time::{Duration, UNIX_EPOCH},
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum SquashFuseError {
    #[error("Squashfs: {0}")]
    Squashfs(#[from] squashfs_ng::SquashfsError),
    #[error("Squashfs lib: {0}")]
    SquashfsLib(#[from] squashfs_ng::LibError),
    #[error("{0}")]
    Custom(String)
}

pub struct SquashfsFilesystem {
    archive: squashfs_ng::read::Archive,
}

impl SquashfsFilesystem {
    pub fn new(archive: squashfs_ng::read::Archive) -> Self {
        Self { archive }
    }

    fn getinode(&self, ino: u64) -> Option<squashfs_ng::read::Node> {
        self.archive.get_id(ino).ok()
    }

    fn convert_to_fuse_file_type(
        node: &squashfs_ng::read::Node,
    ) -> Result<fuser::FileType, SquashFuseError> {
        if node.is_dir()? {
            Ok(fuser::FileType::Directory)
        } else if node.is_file()? {
            Ok(fuser::FileType::RegularFile)
        } else {
            Err(SquashFuseError::Custom("Node is not a file or a directory".into()))
        }
    }

    fn node_to_attr(ino: u64, node: &squashfs_ng::read::Node) -> Result<fuser::FileAttr, SquashFuseError> {
        match node.data().unwrap() {
            squashfs_ng::read::Data::File(s) => {
                return Ok(fuser::FileAttr {
                    ino,
                    size: s.size() as u64,
                    blocks: s.size() as u64 / 512,
                    atime: UNIX_EPOCH,
                    mtime: UNIX_EPOCH,
                    ctime: UNIX_EPOCH,
                    crtime: UNIX_EPOCH,
                    kind: SquashfsFilesystem::convert_to_fuse_file_type(node)?,
                    perm: 0o755,
                    nlink: 1,
                    uid: 0,
                    gid: 0,
                    rdev: 0,
                    flags: 0,
                    blksize: 512,
                });
            }
            _ => Err(SquashFuseError::Custom("Not a file".into())),
        }
    }
}

impl fuser::Filesystem for SquashfsFilesystem {
    fn lookup(
        &mut self,
        _req: &fuser::Request,
        parent: u64,
        name: &OsStr,
        reply: fuser::ReplyEntry,
    ) {
        if let Some(parent_node) = self.getinode(parent) {
            if let squashfs_ng::read::Data::Dir(ref dir) = parent_node.data().unwrap() {
                if let Ok(Some(child_node)) = dir.child(name.to_str().unwrap()) {
                    let ino = child_node.id() as u64;
                    let attr = SquashfsFilesystem::node_to_attr(ino, &child_node).unwrap();
                    reply.entry(&Duration::new(1, 0), &attr, 0);
                    return;
                }
            }
        }
        reply.error(libc::ENOENT);
    }

    fn getattr(&mut self, _req: &fuser::Request, ino: u64, reply: fuser::ReplyAttr) {
        if let Some(node) = self.getinode(ino) {
            let attr = SquashfsFilesystem::node_to_attr(ino, &node).unwrap();
            reply.attr(&Duration::new(1, 0), &attr);
        } else {
            reply.error(libc::ENOENT);
        }
    }

    fn opendir(&mut self, _req: &fuser::Request, ino: u64, _flagss: i32, reply: fuser::ReplyOpen) {
        if let Some(node) = self.getinode(ino) {
            if node.is_dir().unwrap() {
                reply.opened(ino, 0);
                return;
            }
        }
        reply.error(libc::ENOTDIR);
    }

    fn readdir(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        if let Some(node) = self.getinode(ino) {
            if let squashfs_ng::read::Data::Dir(dir) = node.data().unwrap() {
                let mut entries = vec![
                    (1, fuser::FileType::Directory, ".".to_string()),
                    (1, fuser::FileType::Directory, "..".to_string()),
                ];

                for entry in dir {
                    let entry = entry.unwrap();
                    let file_type = SquashfsFilesystem::convert_to_fuse_file_type(&entry).unwrap();
                    entries.push((
                        entry.id() as u64,
                        file_type,
                        entry.name().unwrap().to_string(),
                    ));
                }

                for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
                    let _ = reply.add(entry.0, i as i64 + 1, entry.1, entry.2);
                }
                reply.ok();
                return;
            }
        }
        reply.error(libc::ENOTDIR);
    }

    fn open(&mut self, _req: &fuser::Request, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        if let Some(node) = self.getinode(ino) {
            if node.is_file().unwrap() {
                reply.opened(ino, 0);
                return;
            }
        }
        reply.error(libc::ENOENT);
    }

    fn read(
        &mut self,
        _req: &fuser::Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        if let Ok(node) = self.archive.get_id(ino) {
            if let Ok(mut file) = node.as_file() {
                let mut buffer = vec![0; size as usize];
                match file.read_to_end(&mut buffer) {
                    Ok(_) => {
                        // Calculate the starting point and the length to read
                        let start = offset as usize;
                        let end = std::cmp::min(start + size as usize, buffer.len());

                        // Send the requested data to FUSE
                        if start < end {
                            reply.data(&buffer[start..end]);
                        } else {
                            reply.data(&[]);
                        }
                    }
                    Err(_) => {
                        reply.error(libc::EIO);
                    }
                }
            } else {
                reply.error(libc::EISDIR); // Inode is a directory, not a file
            }
        } else {
            reply.error(libc::ENOENT); // Inode not found
        }
    }
}
