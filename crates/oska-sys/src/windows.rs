//! [`Dir`] on Windows: `NtCreateFile` relative to a directory handle, one
//! component at a time, never following a reparse point.
//!
//! `NtCreateFile` takes the handle of a root directory and a name to open
//! beneath it, which is the only race-free way Windows has to open a path
//! component by component. Every name is opened with
//! `FILE_OPEN_REPARSE_POINT`, so a reparse point (a symlink, a junction or
//! a mount point) is opened itself rather than followed, and is then
//! refused from its attributes, read from the open handle. A file is also
//! refused when it has more than one link. The refusals are worded as on
//! Unix, where a reparse point's place is taken by a symlink.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;

/// The declarations from `ntdll` this crate uses, with their values from
/// the Windows SDK's headers (`winternl.h`, `ntifs.h`, `winnt.h`), for
/// x86_64 Windows, the one Windows target Oska releases for.
mod nt {
    use std::ffi::c_void;

    /// A `HANDLE`.
    pub type Handle = *mut c_void;

    /// An `NTSTATUS`: negative for an error, as `NT_SUCCESS` tests.
    pub type NtStatus = i32;

    /// `UNICODE_STRING`: UTF-16, not NUL-terminated, its length in bytes.
    #[repr(C)]
    pub struct UnicodeString {
        pub length: u16,
        pub maximum_length: u16,
        pub buffer: *const u16,
    }

    /// `OBJECT_ATTRIBUTES`: what to open, a name beneath `root_directory`.
    #[repr(C)]
    pub struct ObjectAttributes {
        pub length: u32,
        pub root_directory: Handle,
        pub object_name: *const UnicodeString,
        pub attributes: u32,
        pub security_descriptor: *mut c_void,
        pub security_quality_of_service: *mut c_void,
    }

    /// `IO_STATUS_BLOCK`: the status, a pointer-sized union, and the
    /// number of bytes an operation returned.
    #[repr(C)]
    #[derive(Default)]
    pub struct IoStatusBlock {
        pub status: usize,
        pub information: usize,
    }

    /// `FILE_BASIC_INFORMATION`: four times, then the attributes.
    #[repr(C)]
    #[derive(Default)]
    pub struct FileBasicInformation {
        pub creation_time: i64,
        pub last_access_time: i64,
        pub last_write_time: i64,
        pub change_time: i64,
        pub file_attributes: u32,
    }

    /// `FILE_STANDARD_INFORMATION`.
    #[repr(C)]
    #[derive(Default)]
    pub struct FileStandardInformation {
        pub allocation_size: i64,
        pub end_of_file: i64,
        pub number_of_links: u32,
        pub delete_pending: u8,
        pub directory: u8,
    }

    // Linked by name, with no import library: `ntdll` is in every Windows
    // process.
    #[link(name = "ntdll", kind = "raw-dylib")]
    extern "system" {
        /// `NtCreateFile`: open or create `object_attributes`' name
        /// beneath its root directory, writing the new handle to
        /// `file_handle`.
        #[link_name = "NtCreateFile"]
        pub fn create_file(
            file_handle: *mut Handle,
            desired_access: u32,
            object_attributes: *const ObjectAttributes,
            io_status_block: *mut IoStatusBlock,
            allocation_size: *const i64,
            file_attributes: u32,
            share_access: u32,
            create_disposition: u32,
            create_options: u32,
            ea_buffer: *mut c_void,
            ea_length: u32,
        ) -> NtStatus;

        /// `NtQueryInformationFile`: write `length` bytes at most of the
        /// information of class `class` about `file_handle`.
        #[link_name = "NtQueryInformationFile"]
        pub fn query_information_file(
            file_handle: Handle,
            io_status_block: *mut IoStatusBlock,
            file_information: *mut c_void,
            length: u32,
            class: u32,
        ) -> NtStatus;

        /// `NtQueryDirectoryFile`, synchronously: write the next entries
        /// of the directory `file_handle` to `file_information`, from its
        /// first when `restart_scan` is set.
        #[link_name = "NtQueryDirectoryFile"]
        pub fn query_directory_file(
            file_handle: Handle,
            event: Handle,
            apc_routine: *mut c_void,
            apc_context: *mut c_void,
            io_status_block: *mut IoStatusBlock,
            file_information: *mut c_void,
            length: u32,
            class: u32,
            return_single_entry: u8,
            file_name: *const UnicodeString,
            restart_scan: u8,
        ) -> NtStatus;

        /// `RtlNtStatusToDosError`: the Win32 error code for a status.
        #[link_name = "RtlNtStatusToDosError"]
        pub fn status_to_error(status: NtStatus) -> u32;
    }

    // Access rights.
    pub const FILE_LIST_DIRECTORY: u32 = 0x0001;
    pub const FILE_TRAVERSE: u32 = 0x0020;
    pub const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    pub const SYNCHRONIZE: u32 = 0x0010_0000;
    pub const FILE_GENERIC_READ: u32 = 0x0012_0089;
    pub const FILE_GENERIC_WRITE: u32 = 0x0012_0116;

    /// `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`: as on
    /// Unix, an open handle keeps no one from renaming or deleting.
    pub const FILE_SHARE_ALL: u32 = 0x7;

    // Dispositions.
    pub const FILE_OPEN: u32 = 1;
    pub const FILE_CREATE: u32 = 2;
    pub const FILE_OPEN_IF: u32 = 3;

    // Options.
    pub const FILE_DIRECTORY_FILE: u32 = 0x0000_0001;
    pub const FILE_SYNCHRONOUS_IO_NONALERT: u32 = 0x0000_0020;
    pub const FILE_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    /// `OBJ_CASE_INSENSITIVE`: names match as Windows' own opens match
    /// them.
    pub const OBJ_CASE_INSENSITIVE: u32 = 0x40;

    // Attributes.
    pub const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    pub const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

    /// `FILE_FLAG_BACKUP_SEMANTICS`, which `CreateFileW` needs to open a
    /// directory.
    pub const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;

    // Information classes.
    pub const FILE_BASIC_INFORMATION: u32 = 4;
    pub const FILE_STANDARD_INFORMATION: u32 = 5;
    pub const FILE_NAMES_INFORMATION: u32 = 12;

    /// `ERROR_DIRECTORY`, the Win32 error for a directory expected and
    /// not found.
    pub const ERROR_DIRECTORY: i32 = 267;

    // Statuses, as the `NTSTATUS` bits.
    pub const STATUS_NO_MORE_FILES: NtStatus = 0x8000_0006_u32 as NtStatus;
    pub const STATUS_NO_SUCH_FILE: NtStatus = 0xC000_000F_u32 as NtStatus;

    /// The access a directory is opened with: to list it, to open names
    /// beneath it, and to read its attributes.
    pub const DIRECTORY: u32 =
        FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

    /// The access a file is opened with to read it.
    pub const FILE_READ: u32 = FILE_GENERIC_READ;

    /// The access a file is opened with to write it, and to read its
    /// attributes.
    pub const FILE_WRITE: u32 = FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES;

    /// The access a name is opened with only to see what it is.
    pub const LOOK: u32 = FILE_READ_ATTRIBUTES | SYNCHRONIZE;
}

/// An open directory: the root of a confined capability, or a directory
/// beneath one. Dropping it closes the handle.
#[derive(Debug)]
pub struct Dir {
    handle: OwnedHandle,
}

impl Dir {
    /// Open the directory at `path`, relative to the working directory
    /// when it is not absolute, following reparse points in every
    /// component as any open by path does: `path` is the caller's trusted
    /// choice.
    ///
    /// A caller that must refuse a reparse point as the last component
    /// opens the parent with this, then the last name with
    /// [`Dir::open_dir`] or [`Dir::open_file`].
    pub fn open(path: &Path) -> io::Result<Dir> {
        let named = |error: io::Error| {
            io::Error::new(error.kind(), format!("`{}`: {error}", path.display()))
        };
        if path.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an empty path names no directory",
            ));
        }
        let file = std::fs::OpenOptions::new()
            .access_mode(nt::DIRECTORY)
            .share_mode(nt::FILE_SHARE_ALL)
            .custom_flags(nt::FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .map_err(named)?;
        let handle = OwnedHandle::from(file);
        if !info(&handle).map_err(named)?.is_dir() {
            return Err(named(io::Error::from_raw_os_error(nt::ERROR_DIRECTORY)));
        }
        Ok(Dir { handle })
    }

    /// The directory reached by walking `names` beneath this one, each a
    /// directory and none a reparse point. With no names, a new handle
    /// for this directory.
    ///
    /// Each name must be one plain component: not empty, `.` or `..`, and
    /// holding no `/`, `\`, `:` or NUL. The caller resolves `..` on the
    /// path as written, before the walk, so the walk never climbs.
    pub fn open_dir<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<Dir> {
        let Some((last, parents)) = names.split_last() else {
            return Ok(Dir {
                handle: self.handle.try_clone()?,
            });
        };
        let parent = self.walk(parents)?;
        let handle = open_dir_name(parent.as_ref().unwrap_or(self), last.as_ref(), names)?;
        Ok(Dir { handle })
    }

    /// The regular file reached by walking `names` beneath this one, open
    /// for reading: every name before the last a directory, and none a
    /// reparse point. Names are as for [`Dir::open_dir`]; there must be
    /// one.
    pub fn open_file<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<File> {
        let (last, parents) = names.split_last().ok_or_else(no_name)?;
        let parent = self.walk(parents)?;
        let (handle, info) = open_name(
            parent.as_ref().unwrap_or(self),
            last.as_ref(),
            Open::FILE_READ,
            names,
        )?;
        lone_file(handle, info, names)
    }

    /// The regular file reached by walking `names` beneath this one, open
    /// for writing and emptied: created when nothing is there. Every name
    /// before the last is a directory, and none is a reparse point, the
    /// last included, so a write never goes through one. Names are as for
    /// [`Dir::open_dir`]; there must be one.
    pub fn create_file<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<File> {
        let (last, parents) = names.split_last().ok_or_else(no_name)?;
        let parent = self.walk(parents)?;
        let (handle, info) = open_name(
            parent.as_ref().unwrap_or(self),
            last.as_ref(),
            Open::FILE_WRITE,
            names,
        )?;
        // Emptied only once it is known to be a regular file with one
        // link: never a directory, or a file outside reached by a hard
        // link.
        let file = lone_file(handle, info, names)?;
        file.set_len(0)?;
        Ok(file)
    }

    /// Create the directory reached by walking `names` beneath this one.
    /// A directory already there is kept; anything else there, a reparse
    /// point included, is an error. Every name before the last is a
    /// directory, and none is a reparse point. Names are as for
    /// [`Dir::open_dir`]; there must be one.
    pub fn create_dir<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<()> {
        let (last, parents) = names.split_last().ok_or_else(no_name)?;
        let walked = self.walk(parents)?;
        let parent = walked.as_ref().unwrap_or(self);
        // `FILE_CREATE` never follows a reparse point at the name: one
        // there, dangling or not, makes it fail as a name already taken.
        // What is there is kept if it is a directory and not a reparse
        // point.
        match create_at(parent, &plain_name(last.as_ref())?, Open::NEW_DIRECTORY) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                open_dir_name(parent, last.as_ref(), names).map(drop)
            }
            Err(error) => Err(refusal(parent, last.as_ref(), error, names)),
        }
    }

    /// The names of the entries of this directory, in the order the
    /// directory gives them, without `.` and `..`. No entry is followed:
    /// a reparse point is listed by its own name.
    pub fn entries(&self) -> io::Result<Vec<OsString>> {
        // A new handle of this directory, opened by the empty name beneath
        // it, so the listing starts at its beginning and moves no other
        // handle's place in it.
        let listing = create_at(self, &[], Open::DIRECTORY)?;
        // `u64`s, so the entries in it are aligned as `ntdll` writes them.
        let mut buffer = vec![0u64; 8192];
        let mut names = Vec::new();
        let length = byte_length(buffer.len() * 8)?;
        let mut restart = 1;
        loop {
            let mut status = nt::IoStatusBlock::default();
            // SAFETY: `listing` is a live directory handle, opened for
            // listing and for synchronous I/O, so the call returns only
            // once it is done. `buffer` is writable for the length given,
            // in bytes, and outlives the call. No event, APC or name to
            // match is passed.
            let result = unsafe {
                nt::query_directory_file(
                    listing.as_raw_handle(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut status,
                    buffer.as_mut_ptr().cast(),
                    length,
                    nt::FILE_NAMES_INFORMATION,
                    0,
                    std::ptr::null(),
                    restart,
                )
            };
            restart = 0;
            if result == nt::STATUS_NO_MORE_FILES || result == nt::STATUS_NO_SUCH_FILE {
                return Ok(names);
            }
            checked(result)?;
            if status.information == 0 {
                return Ok(names);
            }
            let bytes: Vec<u8> = buffer.iter().flat_map(|word| word.to_ne_bytes()).collect();
            let filled = bytes.get(..status.information).ok_or_else(malformed)?;
            for name in entry_names(filled)? {
                if name != "." && name != ".." {
                    names.push(name);
                }
            }
        }
    }

    /// The directory reached by walking `names`, or `None` for this one.
    fn walk<N: AsRef<OsStr>>(&self, names: &[N]) -> io::Result<Option<Dir>> {
        let mut at: Option<Dir> = None;
        for (walked, name) in names.iter().enumerate() {
            let handle = open_dir_name(
                at.as_ref().unwrap_or(self),
                name.as_ref(),
                &names[..=walked],
            )?;
            at = Some(Dir { handle });
        }
        Ok(at)
    }
}

/// How [`create_at`] opens a name: the access asked for, what to do when
/// it exists or not, and whether it must be a directory.
#[derive(Clone, Copy)]
struct Open {
    access: u32,
    disposition: u32,
    options: u32,
}

impl Open {
    /// An existing directory, to walk beneath or list.
    const DIRECTORY: Open = Open {
        access: nt::DIRECTORY,
        disposition: nt::FILE_OPEN,
        options: 0,
    };

    /// A new directory, and an error if anything is at its name.
    const NEW_DIRECTORY: Open = Open {
        access: nt::DIRECTORY,
        disposition: nt::FILE_CREATE,
        options: nt::FILE_DIRECTORY_FILE,
    };

    /// An existing file, to read.
    const FILE_READ: Open = Open {
        access: nt::FILE_READ,
        disposition: nt::FILE_OPEN,
        options: 0,
    };

    /// A file to write, created when nothing is at its name. Not
    /// emptied: [`Dir::create_file`] empties it only once it is known to
    /// be a regular one.
    const FILE_WRITE: Open = Open {
        access: nt::FILE_WRITE,
        disposition: nt::FILE_OPEN_IF,
        options: 0,
    };

    /// Whatever is at a name, only to read its attributes.
    const LOOK: Open = Open {
        access: nt::LOOK,
        disposition: nt::FILE_OPEN,
        options: 0,
    };
}

/// What a handle's attributes say about what it opened.
#[derive(Clone, Copy)]
struct Info {
    attributes: u32,
    links: u32,
}

impl Info {
    fn is_reparse_point(self) -> bool {
        self.attributes & nt::FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    fn is_dir(self) -> bool {
        self.attributes & nt::FILE_ATTRIBUTE_DIRECTORY != 0
    }
}

/// Open the single component `name` beneath `parent` as a directory,
/// refused when it is a reparse point or not a directory. `names` is the
/// walk so far, for messages.
fn open_dir_name<N: AsRef<OsStr>>(
    parent: &Dir,
    name: &OsStr,
    names: &[N],
) -> io::Result<OwnedHandle> {
    let (handle, info) = open_name(parent, name, Open::DIRECTORY, names)?;
    if !info.is_dir() {
        return Err(not_a_directory(&joined(names)));
    }
    Ok(handle)
}

/// Open the single component `name` beneath `parent` as `how` asks,
/// refused when it is a reparse point, with what it is. `names` is the
/// walk so far, for messages.
fn open_name<N: AsRef<OsStr>>(
    parent: &Dir,
    name: &OsStr,
    how: Open,
    names: &[N],
) -> io::Result<(OwnedHandle, Info)> {
    let handle = create_at(parent, &plain_name(name)?, how)
        .map_err(|error| refusal(parent, name, error, names))?;
    let info = info(&handle)?;
    if info.is_reparse_point() {
        return Err(reparse_point(&joined(names)));
    }
    Ok((handle, info))
}

/// `error` from opening `name` beneath `parent`, with the walk so far,
/// `names`, named: a reparse point there said plainly, as an open that
/// asks for more than to look at it may fail for its own reasons.
fn refusal<N: AsRef<OsStr>>(
    parent: &Dir,
    name: &OsStr,
    error: io::Error,
    names: &[N],
) -> io::Error {
    let path = joined(names);
    let there = plain_name(name).and_then(|name| create_at(parent, &name, Open::LOOK));
    match there.and_then(|handle| info(&handle)) {
        Ok(info) if info.is_reparse_point() => reparse_point(&path),
        _ => io::Error::new(error.kind(), format!("`{path}`: {error}")),
    }
}

/// `NtCreateFile` for `name`, UTF-16, beneath `parent` as `how` asks,
/// never following a reparse point, for synchronous I/O. The empty name
/// opens `parent` itself again.
fn create_at(parent: &Dir, name: &[u16], how: Open) -> io::Result<OwnedHandle> {
    let length = u16::try_from(name.len() * 2)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "a file name is too long"))?;
    let object_name = nt::UnicodeString {
        length,
        maximum_length: length,
        buffer: match name.is_empty() {
            true => std::ptr::null(),
            false => name.as_ptr(),
        },
    };
    let attributes = nt::ObjectAttributes {
        length: std::mem::size_of::<nt::ObjectAttributes>() as u32,
        root_directory: parent.handle.as_raw_handle(),
        object_name: &object_name,
        attributes: nt::OBJ_CASE_INSENSITIVE,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut handle: nt::Handle = std::ptr::null_mut();
    let mut status = nt::IoStatusBlock::default();
    // SAFETY: `handle` and `status` are writable, and `attributes` and
    // the name it points to are valid and outlive the call: the name's
    // buffer holds `length` bytes, or is null with a length of 0.
    // `root_directory` is the handle of a live `Dir`, borrowed for the
    // call. No allocation size or extended attributes are passed.
    let result = unsafe {
        nt::create_file(
            &mut handle,
            how.access,
            &attributes,
            &mut status,
            std::ptr::null(),
            nt::FILE_ATTRIBUTE_NORMAL,
            nt::FILE_SHARE_ALL,
            how.disposition,
            how.options | nt::FILE_SYNCHRONOUS_IO_NONALERT | nt::FILE_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
            0,
        )
    };
    checked(result)?;
    // SAFETY: `NtCreateFile` succeeded, so `handle` is a new handle that
    // nothing else owns or will close.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// The attributes and the number of links of what `handle` opened.
fn info(handle: &OwnedHandle) -> io::Result<Info> {
    let mut basic = nt::FileBasicInformation::default();
    let mut status = nt::IoStatusBlock::default();
    // SAFETY: `handle` is live, opened with `FILE_READ_ATTRIBUTES`, and
    // `basic` and `status` are writable for the call, `basic` for the
    // length given.
    let result = unsafe {
        nt::query_information_file(
            handle.as_raw_handle(),
            &mut status,
            (&mut basic as *mut nt::FileBasicInformation).cast(),
            std::mem::size_of::<nt::FileBasicInformation>() as u32,
            nt::FILE_BASIC_INFORMATION,
        )
    };
    checked(result)?;
    let mut standard = nt::FileStandardInformation::default();
    // SAFETY: as above, with `standard`.
    let result = unsafe {
        nt::query_information_file(
            handle.as_raw_handle(),
            &mut status,
            (&mut standard as *mut nt::FileStandardInformation).cast(),
            std::mem::size_of::<nt::FileStandardInformation>() as u32,
            nt::FILE_STANDARD_INFORMATION,
        )
    };
    checked(result)?;
    Ok(Info {
        attributes: basic.file_attributes,
        links: standard.number_of_links,
    })
}

/// `Ok` for a status that is a success, or the error it stands for.
fn checked(status: nt::NtStatus) -> io::Result<()> {
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: `RtlNtStatusToDosError` takes a status by value and only
    // computes the error code for it.
    let code = unsafe { nt::status_to_error(status) };
    Err(io::Error::from_raw_os_error(code as i32))
}

/// The names in `filled`, a run of `FILE_NAMES_INFORMATION` entries as
/// `NtQueryDirectoryFile` wrote them: each the offset of the next, or 0
/// for the last, its index, its name's length in bytes, and its name in
/// UTF-16.
fn entry_names(filled: &[u8]) -> io::Result<Vec<OsString>> {
    let word = |at: usize| -> io::Result<usize> {
        let bytes = filled.get(at..at + 4).ok_or_else(malformed)?;
        Ok(u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    };
    let mut names = Vec::new();
    let mut at = 0;
    while at < filled.len() {
        let next = word(at)?;
        let length = word(at + 8)?;
        let name = filled
            .get(at + 12..at + 12 + length)
            .ok_or_else(malformed)?
            .chunks_exact(2)
            .map(|unit| u16::from_ne_bytes([unit[0], unit[1]]))
            .collect::<Vec<u16>>();
        names.push(OsString::from_wide(&name));
        if next == 0 {
            break;
        }
        at += next;
    }
    Ok(names)
}

/// The error for a directory listing not laid out as documented.
fn malformed() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "a directory listing is not as Windows documents it",
    )
}

/// `bytes` as the `ULONG` length a call takes.
fn byte_length(bytes: usize) -> io::Result<u32> {
    u32::try_from(bytes).map_err(|_| malformed())
}

/// `name` in UTF-16, if it is one plain component: not empty, `.` or
/// `..`, and holding no `/`, `\`, `:` (which would name an alternate data
/// stream) or NUL.
fn plain_name(name: &OsStr) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = name.encode_wide().collect();
    let separator = |unit: &u16| matches!(*unit, 0 | 0x2F | 0x5C | 0x3A);
    if wide.is_empty() || name == "." || name == ".." || wide.iter().any(separator) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{}` is not a single file name", name.to_string_lossy()),
        ));
    }
    Ok(wide)
}

/// The error for a file operation given no name.
fn no_name() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "no file name to open")
}

/// `handle`, opened by walking `names` and described by `info`, as a
/// file, if it is a regular file with only that one name. A file with
/// another hard link may be one outside the directory the walk began in,
/// which a link planted beneath it would otherwise expose, so it is
/// refused. The check is on the open handle, so the file cannot be
/// swapped after it.
fn lone_file<N: AsRef<OsStr>>(handle: OwnedHandle, info: Info, names: &[N]) -> io::Result<File> {
    if info.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{}` is not a regular file", joined(names)),
        ));
    }
    if info.links > 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "`{}` has more than one hard link, and confined access never opens one",
                joined(names)
            ),
        ));
    }
    Ok(File::from(handle))
}

/// The error for a reparse point at `path`, worded as Unix words a
/// symlink.
fn reparse_point(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("`{path}` is a symlink, and confined access never follows one"),
    )
}

/// The error for something not a directory at `path` where the walk
/// needs one, worded as Unix words it.
fn not_a_directory(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!(
            "`{path}` is a symlink or not a directory, and confined access never follows a symlink"
        ),
    )
}

/// `names` joined with `/`, for messages.
fn joined<N: AsRef<OsStr>>(names: &[N]) -> String {
    names
        .iter()
        .map(|name| name.as_ref().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
