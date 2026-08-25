use alloc::string::String;
use crate::debug;
use crate::fs::{vfs::{AccessMode, FileSystem, OpenFlags}, Mode, Path, VFS, FsError};

pub mod elf;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no access")]
    NoAccess,
    #[error(transparent)]
    FsError(FsError),
    #[error("unsupported bimfmt")]
    UnsupportedBinfmt
}

pub fn binfmt_router(path: &str) -> Result<(), Error> {
    let file = VFS
        .force()
        .open(
            &Path::new(path),
            OpenFlags::read_only(),
            Mode::new(),
        )
        .unwrap();

    if !file.dentry.inode.mode.user_execute() {
        return Err(Error::NoAccess);
    }

    let mut prefix = [0; 128];

    file.read(&mut prefix)
        .map_err(Error::FsError)?;

    match prefix {
        [0x7F, 0x45, 0x4c, 0x46, ..] => {
            debug!("this is elf")
        },
        [b'#', b'!', raw_path @ ..] => {
            let mut path = String::new();

            for ch in raw_path {
                if ch == b'\n' {
                    break;
                }
                path.push(ch as char);
            }

            debug!("this is shebang")
        }
        _ => {
            return Err(Error::UnsupportedBinfmt);
        }
    }

    todo!()
}
