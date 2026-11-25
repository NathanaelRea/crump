//! Identity of the running executable image.

use std::{
    fs::File,
    io::{self, Read},
    sync::OnceLock,
};

use sha2::{Digest, Sha256};

/// SHA-256 of the exact executable image this process is running, hashed once per process.
///
/// A rebuild or reinstall replaces the file at the executable's path while this process keeps
/// running the old image. Looking the path up again would then fail, or hash a different build,
/// so the image is opened through the process itself where the platform allows it.
pub(crate) fn sha256() -> io::Result<[u8; 32]> {
    static DIGEST: OnceLock<Result<[u8; 32], (io::ErrorKind, String)>> = OnceLock::new();
    DIGEST
        .get_or_init(|| hash_running_image().map_err(|error| (error.kind(), error.to_string())))
        .clone()
        .map_err(|(kind, message)| {
            io::Error::new(kind, format!("cannot hash the Crump executable: {message}"))
        })
}

/// Lower-case hexadecimal [`sha256`].
pub(crate) fn sha256_hex() -> io::Result<String> {
    Ok(sha256()?.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn hash_running_image() -> io::Result<[u8; 32]> {
    let mut file = match File::open("/proc/self/exe") {
        Ok(file) => file,
        Err(_) => File::open(std::env::current_exe()?)?,
    };
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1_024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_image_hash_is_stable_and_matches_the_file_on_disk() {
        let expected: [u8; 32] =
            Sha256::digest(std::fs::read(std::env::current_exe().unwrap()).unwrap()).into();
        assert_eq!(sha256().unwrap(), expected);
        assert_eq!(sha256().unwrap(), expected);
        assert_eq!(sha256_hex().unwrap().len(), 64);
    }
}
