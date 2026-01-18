use std::{
    io::Read,
    ffi::OsStr,
    num::NonZeroUsize,
    time::{Duration, UNIX_EPOCH},
};


use backhand::{InnerNode, Node, SquashfsFileReader};
use fuser::{ReplyAttr, ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, Request};
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
    verbose: bool,
}

impl<'a> SquashfsFilesystem<'a> {
    pub fn new(archive: backhand::FilesystemReader<'a>, verbose: bool) -> Self {
        if verbose {
            println!("SquashfsFilesystem::verbose enabled");
        }
        Self { archive, verbose }
    }

    fn node_from_ino(&self, ino: usize) -> Option<&Node<SquashfsFileReader>> {
        if ino == 1 {
            // Root inode is always 1
            return Some(&self.archive.root.root());
        }
       
        self.find_node_recursive(NonZeroUsize::new(1).unwrap(), ino)
    }

    fn find_node_recursive(
        &self,
        dir_ino: NonZeroUsize,
        target_ino: usize,
    ) -> Option<&Node<SquashfsFileReader>> {
        for (child_ino_nz, child_node) in self.archive.root.children_of(dir_ino) {
            let child_ino = child_ino_nz.get();

            if child_ino == target_ino {
                return Some(child_node);
            }

            if matches!(child_node.inner, InnerNode::Dir(_)) {
                let child_dir_ino = NonZeroUsize::new(child_ino).unwrap();
                if let Some(found) = self.find_node_recursive(child_dir_ino, target_ino) {
                    return Some(found);
                }
            }
        }

        None
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
        if self.verbose {
            println!("SquashfsFilesystem::getattr()");
        }
        match self.node_from_ino(ino as usize) {
            Some(node) => {
                match &node.inner {
                    InnerNode::File(file) => {
                        let mtime = UNIX_EPOCH + Duration::from_secs(node.header.mtime as u64);

                        reply.attr(
                            &Duration::from_secs(1),
                            &fuser::FileAttr {
                                ino,
                                size: file.file_len() as u64,
                                blocks: ((file.file_len() + 511) / 512) as u64,
                                atime: mtime,
                                mtime, 
                                ctime: mtime,
                                crtime: mtime,
                                kind: fuser::FileType::RegularFile,
                                perm: node.header.permissions,
                                nlink: 1,
                                uid: node.header.uid,
                                gid: node.header.gid,
                                rdev: 0,
                                flags: 0,
                                blksize: 4096,
                            }
                        );
                    },
                    InnerNode::Dir(_) => {
                        let mtime = UNIX_EPOCH + Duration::from_secs(node.header.mtime as u64);

                        reply.attr(
                            &Duration::from_secs(1),
                            &fuser::FileAttr {
                                ino,
                                size: 4096,
                                blocks: 1,
                                atime: mtime,
                                mtime, 
                                ctime: mtime,
                                crtime: mtime,
                                kind: fuser::FileType::Directory,
                                perm: node.header.permissions & !0o77000,
                                nlink: 2,
                                uid: node.header.uid,
                                gid: node.header.gid,
                                flags: 0,
                                rdev: 0,
                                blksize: 4096,
                            }
                        );
                    },
                   _ => reply.error(libc::ENOSYS),
                }
            }
            None => reply.error(libc::ENOENT),
        }
    }

    fn open(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        if self.verbose {
            println!("SquashfsFilesystem::open()");
        }
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
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyData,
    ) {
        if self.verbose {
            println!("SquashfsFilesystem::read(ino={}, offset={}, size={})", ino, offset, size);
        }

        let node = match self.node_from_ino(ino as usize) {
            Some(n) => n,
            None => return reply.error(libc::ENOENT),
        };
    
        if let InnerNode::File(file) = &node.inner {
            let file_size = file.file_len() as u64;
            
            if offset as u64 >= file_size {
                return reply.data(&[]); // EOF
            }
            
            let to_read = std::cmp::min(size as u64, file_size - offset as u64) as usize;
            
            let mut reader = self.archive.file(&file).reader();
            
            // **Emulate seek by reading and discarding bytes**
            // TODO: find a better solution, this code it way too slow!! -> reader caching?
            if offset > 0 {
                let mut discard = vec![0; offset as usize];
                match reader.read_exact(&mut discard) {
                    Ok(_) => {},
                    Err(e) => {
                        eprintln!("Failed to skip to offset {}: {}", offset, e);
                        return reply.error(libc::EINVAL);
                    }
                }
            }
            
            // Now read the actual data
            let mut buf = vec![0; to_read];
            match reader.read_exact(&mut buf) {
                Ok(_) => reply.data(&buf),
                Err(e) => {
                    eprintln!("Read error for ino {}: {}", ino, e);
                    reply.error(libc::EIO);
                }
            }
        } else {
            reply.error(libc::EISDIR);
        }
    }



    fn opendir(&mut self, _req: &Request<'_>, ino: u64, _flags: i32, reply: ReplyOpen) {
        if self.verbose {
            println!("SquashfsFilesystem::opendir()");
        }

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

    fn releasedir(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: i32, reply: ReplyEmpty) {
        if self.verbose {
            println!("SquashfsFilesystem::releasedir()");
        }

        reply.ok();
    }

    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if self.verbose {
            println!("SquashfsFilesystem::lookup(parent={}, name={:?})", parent, name);
        }

        let parent_node = match self.node_from_ino(parent as usize) {
            Some(n) => n,
            None => {
                reply.error(libc::ENOENT);
                return;
            }
        };
        
        let parent_path = &parent_node.fullpath;
        let parent_ino = NonZeroUsize::new(parent as usize).unwrap();
        
        for (child_ino_nz, child_node) in self.archive.root.children_of(parent_ino) {
            // **ROBUST PARENT FILTER**
            let belongs_here = if parent_path == &std::path::Path::new("/") {
                matches!(child_node.fullpath.parent(), Some(p) if p == parent_path)
                    || child_node.fullpath.parent().is_none()
            } else {
                child_node.fullpath.parent() == Some(parent_path)
            };
            
            if !belongs_here {
                continue;
            }
            
            let child_name = child_node.fullpath.file_name()
                .and_then(|n| n.to_str())
                .map(OsStr::new);
            
            if child_name == Some(name) {
                let child_ino = child_ino_nz.get() as u64;
                let mtime = UNIX_EPOCH + Duration::from_secs(child_node.header.mtime as u64);
                
                let attr = fuser::FileAttr {
                    ino: child_ino,
                    size: match &child_node.inner {
                        InnerNode::File(f) => f.file_len() as u64,
                        _ => 4096,
                    },
                    blocks: 1,
                    atime: mtime,
                    mtime,
                    ctime: mtime,
                    crtime: mtime,
                    kind: SquashfsFilesystem::inner_to_fs_type(child_node.inner.clone()),
                    perm: child_node.header.permissions as u16,
                    nlink: 1,
                    uid: child_node.header.uid,
                    gid: child_node.header.gid,
                    rdev: 0,
                    blksize: 4096,
                    flags: 0,
                };
                
                reply.entry(&Duration::from_secs(1), &attr, 0);
                return;
            }
        }
        reply.error(libc::ENOENT);
    }

    
    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if self.verbose {
            println!("SquashfsFilesystem::readdir(ino={}, offset={})", ino, offset);
        }
    
        let node = match self.node_from_ino(ino as usize) {
            Some(n) => n,
            None => return reply.error(libc::ENOENT),
        };
    
        if !matches!(node.inner, InnerNode::Dir(_)) {
            return reply.error(libc::ENOTDIR);
        }
    
        let dir_path = &node.fullpath;
    
        // Add "." and ".." 
        let mut current_offset = offset;
        if current_offset == 0 {
            if reply.add(ino, 1, fuser::FileType::Directory, OsStr::new(".")) {
                reply.ok(); return;
            }
            if reply.add(1, 2, fuser::FileType::Directory, OsStr::new("..")) {
                reply.ok(); return;
            }
            current_offset = 2;
        }
    
        let dir_ino = NonZeroUsize::new(ino as usize).unwrap();
    
        for (i, (child_ino_nz, child_node)) in self
            .archive
            .root
            .children_of(dir_ino)
            .enumerate()
            .skip((current_offset - 2) as usize)
        {
            // ROBUST PATH FILTER - handle all edge cases
            let belongs_here = if dir_path == &std::path::Path::new("/") {
                // Root: accept files with parent "/" or no parent
                matches!(child_node.fullpath.parent(), Some(p) if p == dir_path)
                    || child_node.fullpath.parent().is_none()
            } else {
                // Subdir: accept files whose parent equals this dir
                child_node.fullpath.parent() == Some(dir_path)
            };
    
            if !belongs_here {
                if self.verbose {
                    println!("  SKIP: child={:?} parent={:?} dir={:?}", 
                            child_node.fullpath.file_name(), 
                            child_node.fullpath.parent(), 
                            dir_path);
                }
                continue;
            }
    
            let child_ino = child_ino_nz.get() as u64;
            let name = child_node.fullpath.file_name()
                .and_then(|n| n.to_str())
                .map(OsStr::new)
                .unwrap_or_else(|| OsStr::new("unknown"));
            if self.verbose {
                println!("  ADD: ino={} name={:?}", child_ino, name);
            }
    
            if reply.add(
                child_ino,
                (i as i64) + 3,
                SquashfsFilesystem::inner_to_fs_type(child_node.inner.clone()),
                name,
            ) {
                break;
            }
        }
    
        reply.ok();
    }
}
