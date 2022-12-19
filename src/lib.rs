use anyhow::{anyhow, bail, Error, Result};
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

mod serialise;
use serialise::Deserializer;

#[derive(Debug, FromPrimitive)]
enum WorkerOp {
    IsValidPath = 1,
    HasSubstitutes = 3,
    QueryPathHash = 4,   // obsolete
    QueryReferences = 5, // obsolete
    QueryReferrers = 6,
    AddToStore = 7,
    AddTextToStore = 8, // obsolete since 1.25, Nix 3.0. Use wopAddToStore
    BuildPaths = 9,
    EnsurePath = 10,
    AddTempRoot = 11,
    AddIndirectRoot = 12,
    SyncWithGC = 13,
    FindRoots = 14,
    ExportPath = 16,   // obsolete
    QueryDeriver = 18, // obsolete
    SetOptions = 19,
    CollectGarbage = 20,
    QuerySubstitutablePathInfo = 21,
    QueryDerivationOutputs = 22, // obsolete
    QueryAllValidPaths = 23,
    QueryFailedPaths = 24,
    ClearFailedPaths = 25,
    QueryPathInfo = 26,
    ImportPaths = 27,                // obsolete
    QueryDerivationOutputNames = 28, // obsolete
    QueryPathFromHashPart = 29,
    QuerySubstitutablePathInfos = 30,
    QueryValidPaths = 31,
    QuerySubstitutablePaths = 32,
    QueryValidDerivers = 33,
    OptimiseStore = 34,
    VerifyStore = 35,
    BuildDerivation = 36,
    AddSignatures = 37,
    NarFromPath = 38,
    AddToStoreNar = 39,
    QueryMissing = 40,
    QueryDerivationOutputMap = 41,
    RegisterDrvOutput = 42,
    QueryRealisation = 43,
    AddMultipleToStore = 44,
    AddBuildLog = 45,
    BuildPathsWithResults = 46,
}

const WORKER_MAGIC_1: u64 = 0x6e697863;
const WORKER_MAGIC_2: u64 = 0x6478696f;
const PROTOCOL_VERSION: DaemonVersion = DaemonVersion {
    major: 1,
    minor: 34,
};
const LVL_ERROR: u64 = 0;

/// Signals that the daemon can send to the client.
pub enum StderrSignal {
    Next = 0x6f6c6d67,
    Read = 0x64617461,  // data needed from source
    Write = 0x64617416, // data for sink
    Last = 0x616c7473,
    Error = 0x63787470,
    StartActivity = 0x53545254,
    StopActivity = 0x53544f50,
    Result = 0x52534c54,
}

pub struct NixReadWrite<R, W> {
    pub read: R,
    pub write: W,
}

pub struct StorePathSet {
    // TODO: in nix, they call `parseStorePath` to separate store directory from path
    paths: Vec<Vec<u8>>,
}

pub struct ValidPathInfo {
    path: Vec<u8>,
}

pub struct FramedData {
    data: Vec<Vec<u8>>,
}

impl ValidPathInfo {
    pub fn write<R: Read, W: Write>(
        &self,
        rw: &mut NixReadWrite<R, W>,
        include_path: bool,
    ) -> Result<()> {
        if include_path {
            rw.write_string(&self.path)?;
        }
        rw.write_string(b"")?; // deriver
        rw.write_string(b"0000000000000000000000000000000000000000000000000000000000000000")?; // narhash
        rw.write_u64(0)?; // number of references
                          // write the references here
        rw.write_u64(0)?; // registrationTime
        rw.write_u64(32)?; // narSize
        rw.write_u64(true as u64)?; // ultimate (built locally?)
        rw.write_u64(0)?; // sigs (first is number of strings, which we set to 0)
        rw.write_string(b"")?; // content addressed address (empty string if input addressed)
        Ok(())
    }
}

impl<R: Read, W: Write> NixReadWrite<R, W> {
    pub fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        self.read.read_exact(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    pub fn read_bool(&mut self) -> Result<bool> {
        self.read_u64().map(|i| i != 0)
    }

    pub fn read_framed_data(&mut self) -> Result<()> {
        loop {
            let len = self.read_u64()?;
            if len == 0 {
                break;
            }
            let mut buf = vec![0; len as usize];
            self.read.read_exact(&mut buf)?;
        }
        Ok(())
    }

    pub fn read_string(&mut self) -> Result<Vec<u8>> {
        // possible errors:
        // Unexecpted EOF
        // IO Error
        // out of memory
        let len = self.read_u64()? as usize;

        // FIXME don't initialize
        let mut buf = vec![0; len];
        self.read.read_exact(&mut buf)?;

        if len % 8 > 0 {
            let padding = 8 - len % 8;
            let mut pad_buf = [0; 8];
            self.read.read_exact(&mut pad_buf[..padding])?;
        }

        Ok(buf)
    }

    pub fn read_store_path_set(&mut self) -> Result<StorePathSet> {
        let len = self.read_u64()?;
        let mut ret = vec![];
        for _ in 0..len {
            ret.push(self.read_string()?);
        }
        Ok(StorePathSet { paths: ret })
    }

    fn write_u64(&mut self, n: u64) -> Result<()> {
        self.write.write(&n.to_le_bytes())?;
        Ok(())
    }

    fn write_string(&mut self, s: &[u8]) -> Result<()> {
        self.write_u64(s.len() as _)?;
        self.write.write_all(&s)?;

        if s.len() % 8 > 0 {
            let padding = 8 - s.len() % 8;
            let pad_buf = [0; 8];
            self.write.write_all(&pad_buf[..padding])?;
        }

        Ok(())
    }

    fn deser<'a>(&'a mut self) -> Deserializer<'a> {
        Deserializer {
            read: &mut self.read,
        }
    }

    fn read_command(&mut self) -> Result<()> {
        eprintln!("read_command");
        let op = self.read_u64()?;
        eprintln!("op: {op:x}");
        let Some(op) = WorkerOp::from_u64(op) else {
            todo!("handle bad worker op");
        };

        match op {
            // TODO: use our new deserializer to read a SetOptions.
            WorkerOp::SetOptions => {
                let keep_failing = self.read_u64()?;
                let keep_going = self.read_u64()?;
                let try_fallback = self.read_u64()?;
                let verbosity = self.read_u64()?;
                let max_build_jobs = self.read_u64()?;
                let max_silent_time = self.read_u64()?;
                let _use_build_hook = self.read_u64()?;
                let verbose_build = LVL_ERROR == self.read_u64()?;
                let _log_type = self.read_u64()?;
                let _print_build_trace = self.read_u64()?;
                let build_cores = self.read_u64()?;
                let use_substitutes = self.read_u64()?;

                let options = Options {
                    keep_failing,
                    keep_going,
                    try_fallback,
                    verbosity,
                    max_build_jobs,
                    max_silent_time,
                    verbose_build,
                    build_cores,
                    use_substitutes,
                };

                eprintln!("{options:#?}");

                let n = self.read_u64()?;
                for _ in 0..n {
                    let name = String::from_utf8(self.read_string()?).unwrap();
                    let value = String::from_utf8(self.read_string()?).unwrap();
                    eprintln!("override: {name} = {value}");
                }
            }
            WorkerOp::AddTempRoot => {
                let path = self.read_string()?;
                eprintln!("AddTempRoot: {}", String::from_utf8_lossy(&path));
                // TODO: implement drop for some logger rather than manually calling this
                self.write_u64(StderrSignal::Last as u64)?; // Send startup messages to the client
                self.write_u64(1)?;
                self.write.flush()?;
            }
            WorkerOp::IsValidPath => {
                let path = self.read_string()?;
                eprintln!("IsValidPath: {}", String::from_utf8_lossy(&path));
                // TODO: implement drop for some logger rather than manually calling this
                self.write_u64(StderrSignal::Last as u64)?; // Send startup messages to the client
                self.write_u64(true as u64)?; // if false, we get AddToStoreNar
                self.write.flush()?;
            }
            WorkerOp::AddToStore => {
                let name = self.read_string()?;
                let cam_str = self.read_string()?;
                let refs = self.read_store_path_set()?;
                let repair = self.read_bool()?;
                eprintln!(
                    "AddToStore: {} / {}",
                    String::from_utf8_lossy(&name),
                    String::from_utf8_lossy(&cam_str)
                );
                self.read_framed_data()?;
                // TODO: implement drop for some logger rather than manually calling this
                self.write_u64(StderrSignal::Last as u64)?; // Send startup messages to the client

                ValidPathInfo { path: name }.write(self, true)?;

                self.write.flush()?;
            }
            WorkerOp::QueryPathInfo => {
                let path = self.read_string()?;
                eprintln!("QueryPathInfo: {}", String::from_utf8_lossy(&path));
                // TODO: implement drop for some logger rather than manually calling this
                self.write_u64(StderrSignal::Last as u64)?; // Send startup messages to the client
                self.write_u64(1)?;
                ValidPathInfo { path }.write(self, false)?;
                self.write.flush()?;
            }
            op => bail!("received worker op: {:?}", op),
        }

        Ok(())
    }

    /// Process a remote nix connection.
    /// Reimplement Daemon::processConnection from nix/src/libstore/daemon.cc
    pub fn process_connection(&mut self) -> Result<()> {
        let magic = self.read_u64()?;
        if magic != WORKER_MAGIC_1 {
            eprintln!("{magic:x}");
            eprintln!("{WORKER_MAGIC_1:x}");
            todo!("handle error: protocol mismatch 1");
        }

        self.write_u64(WORKER_MAGIC_2)?;
        self.write_u64(PROTOCOL_VERSION.into())?;
        self.write.flush()?;

        let client_version = self.read_u64()?;

        if client_version < 0x10a {
            eprintln!("Client version {client_version} is too old");
            todo!("handle error: client version");
        }

        // TODO keep track of number of WorkerOps performed
        let mut _op_count: u64 = 0;

        let daemon_version = DaemonVersion::from(client_version);

        if daemon_version.minor >= 14 {
            let _obsolete_cpu_affinity = self.read_u64()?;
        }

        if daemon_version.minor >= 11 {
            let _obsolete_reserve_space = self.read_u64()?;
        }

        if daemon_version.minor >= 33 {
            // TODO figure out what we need to set as the version
            self.write_string("rust-nix-bazel-0.1.0".as_bytes())?;
        }
        self.write_u64(StderrSignal::Last as u64)?; // Send startup messages to the client
        self.write.flush()?;

        loop {
            // TODO process worker ops
            self.read_command()?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SetOptions {
    pub keep_failing: u64,
    pub keep_going: u64,
    pub try_fallback: u64,
    pub verbosity: u64,
    pub max_build_jobs: u64,
    pub max_silent_time: u64,
    _use_build_hook: u64,
    pub build_verbosity: u64,
    _log_type: u64,
    _print_build_trace: u64,
    pub build_cores: u64,
    pub use_substitutes: u64,
    pub options: Vec<(Vec<u8>, Vec<u8>)>,
}

#[derive(Debug)]
struct Options {
    keep_failing: u64,
    keep_going: u64,
    try_fallback: u64,
    verbosity: u64,
    max_build_jobs: u64,
    max_silent_time: u64,
    verbose_build: bool,
    build_cores: u64,
    use_substitutes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DaemonVersion {
    major: u8,
    minor: u8,
}

impl From<u64> for DaemonVersion {
    fn from(x: u64) -> Self {
        let major = ((x >> 8) & 0xff) as u8;
        let minor = (x & 0xff) as u8;
        Self { major, minor }
    }
}

impl From<DaemonVersion> for u64 {
    fn from(DaemonVersion { major, minor }: DaemonVersion) -> Self {
        ((major as u64) << 8) | minor as u64
    }
}
