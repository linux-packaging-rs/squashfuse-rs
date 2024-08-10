use std::{
    env, fs::{self, create_dir, File}, io::{BufReader, Write}, path::{Path, PathBuf}, process::Command
};

fn cleanup(tmp_dir: &Path, out_dir: &Path, mount_dir: &Path) {
    let _ = fs::remove_dir_all(tmp_dir.join("test_archive"));
    let _ = fs::remove_dir_all(out_dir);
    let _ = fs::remove_dir_all(mount_dir);
    let _ = Command::new("umount").arg(mount_dir).status();
}

fn create_archive(tmp_dir: &Path, out: &Path) -> anyhow::Result<PathBuf> {
    create_dir(tmp_dir.join("test_archive"))?;
    File::create_new(tmp_dir.join("test_archive").join("stf.txt"))?
        .write_fmt(format_args!("Hey!!! waOIDPOAWIDPOAWPOi"))?;
    File::create_new(tmp_dir.join("test_archive").join("other.w"))?
        .write_fmt(format_args!("Here's some stuff"))?;
    create_dir(tmp_dir.join("test_archive").join("nested_folder"))?;
    File::create_new(
        tmp_dir
            .join("test_archive")
            .join("nested_folder")
            .join("other.w2"),
    )?
    .write_fmt(format_args!("ddd's some stuff"))?;
    // create output dir
    let _ = create_dir(out);
    let output = Command::new("mksquashfs")
        .arg(tmp_dir.join("test_archive"))
        .arg(out.join("test_archive.squashfs"))
        .output()?;

    if !out.join("test_archive.squashfs").exists() {
        Err(anyhow::anyhow!(
            "mksquashfs failed\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    } else {
        Ok(out.join("test_archive.squashfs"))
    }
}

fn main() -> anyhow::Result<()> {
    let tmp_dir = std::env::current_dir().unwrap();
    let out_dir = std::env::current_dir().unwrap().join("out");
    let mount_dir = Path::new("/tmp/.test_mount");
    if env::args().nth(1).is_some_and(|e| e == "--clean") {
        println!("Cleaning mount and output!");
        cleanup(&tmp_dir, &out_dir, &mount_dir);
        return Ok(());
    }
    cleanup(&tmp_dir, &out_dir, &mount_dir);
    match mount_filesystem_inner(&tmp_dir, &out_dir, &mount_dir) {
        Ok(()) => {
            cleanup(&tmp_dir, &out_dir, &mount_dir);
            Ok(())
        }
        Err(e) => {
            cleanup(&tmp_dir, &out_dir, &mount_dir);
            Err(e)
        }
    }
}

fn mount_filesystem_inner(tmp_dir: &Path, out_dir: &Path, mount_dir: &Path) -> anyhow::Result<()> {
    // create the archive
    let result = create_archive(&tmp_dir, &out_dir)?;
    // test mounting the archive
    let _ = fs::create_dir(&mount_dir);
    println!("Path created at {:?}", result);
    let reader = BufReader::new(File::open(&result).unwrap());
    println!("Buffered reader created");
    let fs_reader = backhand::FilesystemReader::from_reader(reader).unwrap();
    println!("FS reader created");
    let fs = squashfuse_rs::SquashfsFilesystem::new(fs_reader, true);
    println!("FS created at {:?}", &mount_dir);
    let mount_options = vec![
        fuser::MountOption::FSName("squashfuse".to_string()),
        fuser::MountOption::RO,
    ];
    println!("Mounting FS...");
    fuser::mount2(fs, &mount_dir, &mount_options)?;
    Ok(())
}
