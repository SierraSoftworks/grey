use deno_runtime::{
    deno_fs::{FileSystem, FsDirEntry, FsError, FsFileType, OpenOptions},
    deno_io::fs::{File, FsResult, FsStat},
    deno_permissions::{CheckedPath, CheckedPathBuf},
};
use std::{path::PathBuf, rc::Rc};

#[derive(Debug)]
pub struct NoOpFs;

impl NoOpFs {
    fn no_op_fs_error<T>(&self) -> FsResult<T> {
        Err(FsError::NotSupported)
    }
}

#[async_trait::async_trait(?Send)]
impl FileSystem for NoOpFs {
    fn cwd(&self) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    fn tmp_dir(&self) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    fn chdir(&self, _path: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn umask(&self, _mask: Option<u32>) -> FsResult<u32> {
        self.no_op_fs_error()
    }

    fn open_sync(&self, _path: &CheckedPath, _options: OpenOptions) -> FsResult<Rc<dyn File>> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn open_async(
        &self,
        _path: CheckedPathBuf,
        _options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.no_op_fs_error()
    }

    fn mkdir_sync(
        &self,
        _path: &CheckedPath,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn mkdir_async(
        &self,
        _path: CheckedPathBuf,
        _recursive: bool,
        _mode: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn chmod_sync(&self, _path: &CheckedPath, _mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn chmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn chown_sync(
        &self,
        _path: &CheckedPath,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn chown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn lchmod_sync(&self, _path: &CheckedPath, _mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lchmod_async(&self, _path: CheckedPathBuf, _mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn lchown_sync(
        &self,
        _path: &CheckedPath,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lchown_async(
        &self,
        _path: CheckedPathBuf,
        _uid: Option<u32>,
        _gid: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn remove_sync(&self, _path: &CheckedPath, _recursive: bool) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn remove_async(&self, _path: CheckedPathBuf, _recursive: bool) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn copy_file_sync(&self, _oldpath: &CheckedPath, _newpath: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn copy_file_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn cp_sync(&self, _path: &CheckedPath, _new_path: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn cp_async(&self, _path: CheckedPathBuf, _new_path: CheckedPathBuf) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn stat_sync(&self, _path: &CheckedPath) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn stat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    fn lstat_sync(&self, _path: &CheckedPath) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lstat_async(&self, _path: CheckedPathBuf) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    fn realpath_sync(&self, _path: &CheckedPath) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn realpath_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    fn read_dir_sync(&self, _path: &CheckedPath) -> FsResult<Vec<FsDirEntry>> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn read_dir_async(&self, _path: CheckedPathBuf) -> FsResult<Vec<FsDirEntry>> {
        self.no_op_fs_error()
    }

    fn rename_sync(&self, _oldpath: &CheckedPath, _newpath: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn rename_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn link_sync(&self, _oldpath: &CheckedPath, _newpath: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn link_async(&self, _oldpath: CheckedPathBuf, _newpath: CheckedPathBuf) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn symlink_sync(
        &self,
        _oldpath: &CheckedPath,
        _newpath: &CheckedPath,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn symlink_async(
        &self,
        _oldpath: CheckedPathBuf,
        _newpath: CheckedPathBuf,
        _file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn read_link_sync(&self, _path: &CheckedPath) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn read_link_async(&self, _path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    fn truncate_sync(&self, _path: &CheckedPath, _len: u64) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn truncate_async(&self, _path: CheckedPathBuf, _len: u64) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn utime_sync(
        &self,
        _path: &CheckedPath,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn utime_async(
        &self,
        _path: CheckedPathBuf,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn lutime_sync(
        &self,
        _path: &CheckedPath,
        _atime_secs: i64,
        _atime_nanos: u32,
        _mtime_secs: i64,
        _mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        mismatched_lifetime_syntaxes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lutime_async(
        &self,
        __path: CheckedPathBuf,
        __atime_secs: i64,
        __atime_nanos: u32,
        __mtime_secs: i64,
        __mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }
}
