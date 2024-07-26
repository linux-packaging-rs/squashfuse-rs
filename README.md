# squashfuse-rs

Implementation of FUSE for SquashFS in rust using the [fuser](https://docs.rs/fuser/latest/fuser/index.html) and [backhand](https://github.com/wcampbell0x2a/backhand) crates!

Inspired by a C library called squashfuse <https://github.com/vasi/squashfuse/tree/master>

PRs are welcome, I don't claim to know a lot about SquashFS and FUSE, so if there is a better way of doing things, please let me know!

Example usage from [appimage-type2-runtime-rs](https://github.com/linux-packaging-rs/appimage-type2-runtime-rs):

```rust
fn fusefs_main(offset: u64, mountpoint: &Path, archive_path: &Path) -> anyhow::Result<()> {
    let reader = BufReader::new(File::open(&archive_path)?);
    let fs = squashfuse_rs::SquashfsFilesystem::new(
        backhand::FilesystemReader::from_reader_with_offset(reader, offset)?,
    );
    let mount_options = vec![
        fuser::MountOption::FSName("squashfuse".to_string()),
        fuser::MountOption::RO,
    ];
    match fuser::mount2(fs, mountpoint, &mount_options) {
        Ok(()) => {
            println!("Mounted {:?} at {:?}", archive_path, mountpoint);
            Ok(())
        }
        Err(err) => Err(anyhow::anyhow!("Failed to mount: {}", err)),
}
```