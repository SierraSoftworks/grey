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

    fn chdir(&self, path: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn umask(&self, mask: Option<u32>) -> FsResult<u32> {
        self.no_op_fs_error()
    }

    fn open_sync(&self, path: &CheckedPath, options: OpenOptions) -> FsResult<Rc<dyn File>> {
        self.no_op_fs_error()
    }

    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn open_async(
        &self,
        path: CheckedPathBuf,
        options: OpenOptions,
    ) -> FsResult<Rc<dyn File>> {
        self.no_op_fs_error()
    }

    fn mkdir_sync(&self, path: &CheckedPath, recursive: bool, mode: Option<u32>) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn mkdir_async(
        &self,
        path: CheckedPathBuf,
        recursive: bool,
        mode: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn chmod_sync(&self, path: &CheckedPath, mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn chmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn chown_sync(&self, path: &CheckedPath, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn chown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn lchmod_sync(&self, path: &CheckedPath, mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lchmod_async(&self, path: CheckedPathBuf, mode: u32) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn lchown_sync(&self, path: &CheckedPath, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lchown_async(
        &self,
        path: CheckedPathBuf,
        uid: Option<u32>,
        gid: Option<u32>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn remove_sync(&self, path: &CheckedPath, recursive: bool) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn remove_async(&self, path: CheckedPathBuf, recursive: bool) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn copy_file_sync(&self, oldpath: &CheckedPath, newpath: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn copy_file_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn cp_sync(&self, path: &CheckedPath, new_path: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn cp_async(&self, path: CheckedPathBuf, new_path: CheckedPathBuf) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn stat_sync(&self, path: &CheckedPath) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn stat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    fn lstat_sync(&self, path: &CheckedPath) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lstat_async(&self, path: CheckedPathBuf) -> FsResult<FsStat> {
        self.no_op_fs_error()
    }

    fn realpath_sync(&self, path: &CheckedPath) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn realpath_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    fn read_dir_sync(&self, path: &CheckedPath) -> FsResult<Vec<FsDirEntry>> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn read_dir_async(&self, path: CheckedPathBuf) -> FsResult<Vec<FsDirEntry>> {
        self.no_op_fs_error()
    }

    fn rename_sync(&self, oldpath: &CheckedPath, newpath: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn rename_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn link_sync(&self, oldpath: &CheckedPath, newpath: &CheckedPath) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn link_async(&self, oldpath: CheckedPathBuf, newpath: CheckedPathBuf) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn symlink_sync(
        &self,
        oldpath: &CheckedPath,
        newpath: &CheckedPath,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn symlink_async(
        &self,
        oldpath: CheckedPathBuf,
        newpath: CheckedPathBuf,
        file_type: Option<FsFileType>,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn read_link_sync(&self, path: &CheckedPath) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn read_link_async(&self, path: CheckedPathBuf) -> FsResult<PathBuf> {
        self.no_op_fs_error()
    }

    fn truncate_sync(&self, path: &CheckedPath, len: u64) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn truncate_async(&self, path: CheckedPathBuf, len: u64) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn utime_sync(
        &self,
        path: &CheckedPath,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn utime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    fn lutime_sync(
        &self,
        path: &CheckedPath,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }

    #[must_use]
    #[allow(
        elided_named_lifetimes,
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn lutime_async(
        &self,
        path: CheckedPathBuf,
        atime_secs: i64,
        atime_nanos: u32,
        mtime_secs: i64,
        mtime_nanos: u32,
    ) -> FsResult<()> {
        self.no_op_fs_error()
    }
}
