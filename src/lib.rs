use std::{
    io::Read,
    num::NonZeroUsize,
    time::{Duration, UNIX_EPOCH},
};

use backhand::{
    InnerNode, Node, SquashfsFileReader,
};
use fuser::{ReplyAttr, Request};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SquashFuseError {
    #[error("Backhand (squashfs support): {0}")]
    Squashfs(#[from] backhand::BackhandError),
    #[error("{0}")]
    Custom(String),
}

pub struct SquashfsFilesystem<'a> {
    archive: backhand::FilesystemReader<'a>,
}

impl<'a> SquashfsFilesystem<'a> {
    pub fn new(archive: backhand::FilesystemReader<'a>) -> Self {
        Self { archive }
    }

    fn node_from_ino(&self, ino: usize) -> Option<Node<SquashfsFileReader>> {
        self.archive.files().nth(ino).cloned()
    }

    fn inner_to_fs_type(inner: InnerNode<SquashfsFileReader>) -> fuser::FileType {
        match inner {
            InnerNode::File(_) => fuser::FileType::RegularFile,
            InnerNode::Symlink(_) => fuser::FileType::Symlink,
            InnerNode::Dir(_) => fuser::FileType::Directory,
            InnerNode::CharacterDevice(_) => fuser::FileType::CharDevice,
            InnerNode::BlockDevice(_) => fuser::FileType::BlockDevice,
            InnerNode::NamedPipe => fuser::FileType::NamedPipe,
            InnerNode::Socket => fuser::FileType::Socket,
        }
    }
}

impl<'a> fuser::Filesystem for SquashfsFilesystem<'a> {
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        match self.node_from_ino(ino as usize) {
            Some(node) => {
                match node.inner {
                    InnerNode::File(file) => {
                        reply.attr(
                            &Duration::from_secs(1),
                            &fuser::FileAttr {
                                ino,
                                size: file.basic.file_size as u64,
                                blocks: file.basic.block_sizes.len() as u64,
                                atime: UNIX_EPOCH,
                                mtime: UNIX_EPOCH, // node.header.mtime
                                ctime: UNIX_EPOCH,
                                crtime: UNIX_EPOCH,
                                kind: fuser::FileType::RegularFile,
                                perm: node.header.permissions,
                                nlink: 1,
                                uid: node.header.uid,
                                gid: node.header.gid,
                                rdev: 0,
                                flags: 0,
                                blksize: 512,
                            },
                        )
                    }
                    _ => panic!("This code shouldn't be reached"),
                }
            }
            None => reply.error(libc::ENOENT),
        }
    }
    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        if let Some(node) = self.node_from_ino(ino as usize) {
            if let InnerNode::File(_) = node.inner {
                reply.opened(0, 0)
            } else {
                reply.error(libc::ENOENT)
            }
        } else {
            reply.error(libc::ENOENT)
        }
    }
    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        _offset: i64,
        _size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        match self.node_from_ino(ino as usize) {
            Some(node) => match node.inner {
                InnerNode::File(file) => {
                    let mut data = Vec::new();
                    self.archive
                        .file(&file.basic)
                        .reader()
                        .read_to_end(&mut data)
                        .expect("Could not write buffer");
                    reply.data(&data)
                }
                _ => panic!("Trying to read from non-file"),
            },
            None => reply.error(libc::ENOENT),
        }
    }
    fn opendir(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        if let Some(node) = self.node_from_ino(ino as usize) {
            if let InnerNode::Dir(_) = node.inner {
                reply.opened(0, 0)
            } else {
                reply.error(libc::ENOENT)
            }
        } else {
            reply.error(libc::ENOENT)
        }
    }
    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: fuser::ReplyDirectory,
    ) {
        if let Some(node) = self.node_from_ino(ino as usize) {
            if let InnerNode::Dir(_) = node.inner {
                for (i, (child_ino, _)) in self
                    .archive
                    .root
                    .children_of(NonZeroUsize::new(ino as usize).unwrap())
                    .enumerate()
                    .skip(offset as usize)
                {
                    if reply.add(
                        child_ino.get() as u64,
                        (i + 1) as i64,
                        SquashfsFilesystem::inner_to_fs_type(
                            self.node_from_ino(child_ino.get()).unwrap().inner,
                        ),
                        node.fullpath.file_name().unwrap(),
                    ) {
                        break;
                    }
                }
            }
            reply.ok();
        } else {
            reply.error(libc::ENOENT);
        }
    }
}
