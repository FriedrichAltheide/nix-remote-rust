use serde::{Deserialize, Serialize};
use std::io::Read;
use std::io::Write;
use std::ops::{Deref, DerefMut};
use tagged_serde::TaggedSerde;

use crate::nar::Nar;
use crate::{
    serialize::{NixDeserializer, NixSerializer},
    NarHash, NixString, Result, StorePath, StorePathSet, StringSet, ValidPathInfoWithPath,
};
use crate::{DerivedPath, Path, PathSet, Realisation, RealisationSet};

/// A zero-sized marker type. Its job is to mark the expected response
/// type for each worker op.
#[derive(Debug, Serialize, Deserialize)]
pub struct Resp<T> {
    #[serde(skip)]
    marker: std::marker::PhantomData<T>,
}

impl<T> Resp<T> {
    fn ty(&self, v: T) -> T {
        v
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Plain<T>(pub T);

impl<T> Deref for Plain<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct WithFramedSource<T>(pub T);

impl<T> Deref for WithFramedSource<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for WithFramedSource<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub trait Stream {
    fn stream(&self, read: &mut impl Read, write: &mut impl Write) -> anyhow::Result<()>;
}

impl<T> Stream for WithFramedSource<T> {
    fn stream(&self, read: &mut impl Read, write: &mut impl Write) -> anyhow::Result<()> {
        let mut de = crate::serialize::NixDeserializer { read };
        let mut ser = crate::serialize::NixSerializer { write };
        const BUF_SIZE: usize = 4096;
        let mut buf = vec![0; BUF_SIZE];

        loop {
            let mut len = u64::deserialize(&mut de)? as usize;
            eprintln!("streaming {len} bytes");
            (len as u64).serialize(&mut ser)?;
            if len == 0 {
                break;
            }
            while len > 0 {
                let chunk_len = len.min(BUF_SIZE);
                de.read.read_exact(&mut buf[..chunk_len])?;
                ser.write.write_all(&buf[..chunk_len])?;
                len -= chunk_len;
            }
        }
        Ok(())
    }
}

impl<T> Stream for Plain<T> {
    fn stream(&self, _read: &mut impl Read, _write: &mut impl Write) -> anyhow::Result<()> {
        Ok(())
    }
}

/// The different worker ops.
///
/// On the wire, they are represented as the opcode followed by the body.
///
/// The second argument in each variant is a tag denoting the expected return value.
#[derive(Debug, TaggedSerde)]
pub enum WorkerOp {
    #[tagged_serde = 1]
    IsValidPath(Plain<StorePath>, Resp<bool>),
    #[tagged_serde = 6]
    QueryReferrers(Plain<StorePath>, Resp<StorePathSet>),
    #[tagged_serde = 7]
    AddToStore(WithFramedSource<AddToStore>, Resp<ValidPathInfoWithPath>),
    #[tagged_serde = 9]
    BuildPaths(Plain<BuildPaths>, Resp<u64>),
    #[tagged_serde = 10]
    EnsurePath(Plain<StorePath>, Resp<u64>),
    #[tagged_serde = 11]
    AddTempRoot(Plain<StorePath>, Resp<u64>),
    #[tagged_serde = 14]
    FindRoots(Plain<()>, Resp<FindRootsResponse>),
    #[tagged_serde = 19]
    SetOptions(Plain<SetOptions>, Resp<()>),
    #[tagged_serde = 20]
    CollectGarbage(Plain<CollectGarbage>, Resp<CollectGarbageResponse>),
    #[tagged_serde = 23]
    QueryAllValidPaths(Plain<()>, Resp<StorePathSet>),
    #[tagged_serde = 26]
    QueryPathInfo(Plain<StorePath>, Resp<QueryPathInfoResponse>),
    #[tagged_serde = 29]
    QueryPathFromHashPart(Plain<NixString>, Resp<OptionalStorePath>),
    #[tagged_serde = 31]
    QueryValidPaths(Plain<QueryValidPaths>, Resp<StorePathSet>),
    #[tagged_serde = 32]
    QuerySubstitutablePaths(Plain<StorePathSet>, Resp<StorePathSet>),
    #[tagged_serde = 33]
    QueryValidDerivers(Plain<StorePath>, Resp<StorePathSet>),
    #[tagged_serde = 34]
    OptimiseStore(Plain<()>, Resp<u64>),
    #[tagged_serde = 35]
    VerifyStore(Plain<VerifyStore>, Resp<bool>),
    #[tagged_serde = 36]
    BuildDerivation(Plain<BuildDerivation>, Resp<BuildResult>),
    #[tagged_serde = 37]
    AddSignatures(Plain<AddSignatures>, Resp<u64>),
    #[tagged_serde = 38]
    NarFromPath(Plain<StorePath>, Resp<Nar>),
    #[tagged_serde = 39]
    AddToStoreNar(WithFramedSource<AddToStoreNar>, Resp<()>),
    #[tagged_serde = 40]
    QueryMissing(Plain<QueryMissing>, Resp<QueryMissingResponse>),
    #[tagged_serde = 41]
    QueryDerivationOutputMap(Plain<StorePath>, Resp<DerivationOutputMap>),
    #[tagged_serde = 42]
    RegisterDrvOutput(Plain<Realisation>, Resp<()>),
    #[tagged_serde = 43]
    QueryRealisation(Plain<NixString>, Resp<RealisationSet>),
    #[tagged_serde = 44]
    AddMultipleToStore(WithFramedSource<AddMultipleToStore>, Resp<()>),
    #[tagged_serde = 45]
    AddBuildLog(WithFramedSource<AddBuildLog>, Resp<u64>),
    #[tagged_serde = 46]
    BuildPathsWithResults(Plain<BuildPaths>, Resp<Vec<(DerivedPath, BuildResult)>>),
}

macro_rules! for_each_op {
    ($macro_name:ident !) => {
        $macro_name!(
            IsValidPath,
            QueryReferrers,
            AddToStore,
            BuildPaths,
            EnsurePath,
            AddTempRoot,
            FindRoots,
            SetOptions,
            CollectGarbage,
            QueryAllValidPaths,
            QueryPathInfo,
            QueryPathFromHashPart,
            QueryValidPaths,
            QuerySubstitutablePaths,
            QueryValidDerivers,
            OptimiseStore,
            VerifyStore,
            BuildDerivation,
            AddSignatures,
            NarFromPath,
            AddToStoreNar,
            QueryMissing,
            QueryDerivationOutputMap,
            RegisterDrvOutput,
            QueryRealisation,
            AddMultipleToStore,
            AddBuildLog,
            BuildPathsWithResults
        )
    };
}

impl Stream for WorkerOp {
    fn stream(&self, read: &mut impl Read, write: &mut impl Write) -> anyhow::Result<()> {
        eprintln!("streaming worker op");
        macro_rules! stream {
            ($($name:ident),*) => {
                match self {
                    $(WorkerOp::$name(op, _resp) => {
                        op.stream(read, write)?;
                    },)*
                }
            };
        }

        for_each_op!(stream!);
        Ok(())
    }
}

impl WorkerOp {
    pub fn proxy_response(&self, mut read: impl Read, mut write: impl Write) -> Result<()> {
        let mut logging_read = crate::printing_read::PrintingRead {
            buf: Vec::new(),
            inner: &mut read,
        };
        let mut deser = NixDeserializer {
            read: &mut logging_read,
        };
        let mut ser = NixSerializer { write: &mut write };
        let mut dbg_buf = Vec::new();
        let mut dbg_ser = NixSerializer {
            write: &mut dbg_buf,
        };
        macro_rules! respond {
            ($($name:ident),*) => {
                #[allow(unreachable_patterns)]
                match self {
                    // Special case for NarFromPath because the response could be large
                    // and needs to be streamed instead of read into memory.
                    WorkerOp::NarFromPath(_inner, _resp) => {
                      crate::nar::stream(&mut deser.read, &mut ser.write)?;
                    }
                    $(WorkerOp::$name(_inner, resp) => {
                        let reply = resp.ty(<_>::deserialize(&mut deser)?);
                        eprintln!("read reply {reply:?}");

                        reply.serialize(&mut dbg_ser)?;
                        if dbg_buf != logging_read.buf {
                            eprintln!("mismatch!");
                            eprintln!("{:?}", &logging_read.buf[0..500]);
                            eprintln!("{:?}", &dbg_buf[0..500]);
                            panic!();
                        }
                        reply.serialize(&mut ser)?;
                    },)*
                }
            };
        }

        for_each_op!(respond!);
        Ok(())
    }
}

type Time = u64;
type OptionalStorePath = StorePath;

#[derive(Debug, Clone, Copy, TaggedSerde, PartialEq, Eq)]
pub enum Verbosity {
    #[tagged_serde = 0]
    Error,
    #[tagged_serde = 1]
    Warn,
    #[tagged_serde = 2]
    Notice,
    #[tagged_serde = 3]
    Info,
    #[tagged_serde = 4]
    Talkative,
    #[tagged_serde = 5]
    Chatty,
    #[tagged_serde = 6]
    Debug,
    #[tagged_serde = 7]
    Vomit,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct SetOptions {
    pub keep_failing: bool,
    pub keep_going: bool,
    pub try_fallback: bool,
    pub verbosity: Verbosity,
    pub max_build_jobs: u64,
    pub max_silent_time: Time,
    _use_build_hook: u64,
    pub build_verbosity: Verbosity,
    _log_type: u64,
    _print_build_trace: u64,
    pub build_cores: u64,
    pub use_substitutes: bool,
    pub options: Vec<(NixString, NixString)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddToStore {
    pub name: StorePath,
    pub cam_str: StorePath,
    pub refs: StorePathSet,
    pub repair: bool,
}

#[derive(Debug, Clone, Copy, TaggedSerde)]
pub enum BuildMode {
    #[tagged_serde = 0]
    Normal,
    #[tagged_serde = 1]
    Repair,
    #[tagged_serde = 2]
    Check,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildPaths {
    pub paths: Vec<StorePath>,
    pub build_mode: BuildMode,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryMissing {
    pub paths: Vec<StorePath>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryPathInfoResponse {
    pub path: Option<ValidPathInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryMissingResponse {
    pub will_build: StorePathSet,
    pub will_substitute: StorePathSet,
    pub unknown: StorePathSet,
    pub download_size: u64,
    pub nar_size: u64,
}

#[derive(Debug, Clone, Copy, TaggedSerde)]
pub enum BuildStatus {
    #[tagged_serde = 0]
    Built,
    #[tagged_serde = 1]
    Substituted,
    #[tagged_serde = 2]
    AlreadyValid,
    #[tagged_serde = 3]
    PermanentFailure,
    #[tagged_serde = 4]
    InputRejected,
    #[tagged_serde = 5]
    OutputRejected,
    #[tagged_serde = 6]
    TransientFailure,
    #[tagged_serde = 7]
    CachedFailure,
    #[tagged_serde = 8]
    TimedOut,
    #[tagged_serde = 9]
    MiscFailure,
    #[tagged_serde = 10]
    DependencyFailed,
    #[tagged_serde = 11]
    LogLimitExceeded,
    #[tagged_serde = 12]
    NotDeterministic,
    #[tagged_serde = 13]
    ResolvesToAlreadyValid,
    #[tagged_serde = 14]
    NoSubstituters,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildResult {
    pub status: BuildStatus,
    pub error_msg: NixString,
    pub times_built: u64,
    pub is_non_deterministic: bool,
    pub start_time: Time,
    pub stop_time: Time,
    pub built_outputs: DrvOutputs,
}

// TODO: first NixString is a DrvOutput; second is a Realisation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DrvOutputs(pub Vec<(NixString, Realisation)>);

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectGarbage {
    pub action: GcAction,
    pub paths_to_delete: StorePathSet,
    pub ignore_liveness: bool,
    pub max_freed: u64,
    _obsolete0: u64,
    _obsolete1: u64,
    _obsolete2: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DerivationOutputMap {
    pub paths: Vec<(NixString, OptionalStorePath)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectGarbageResponse {
    pub paths: PathSet,
    pub bytes_freed: u64,
    _obsolete: u64,
}

#[derive(Debug, Clone, TaggedSerde, Default)]
pub enum GcAction {
    #[tagged_serde = 0]
    ReturnLive,
    #[tagged_serde = 1]
    ReturnDead,
    #[default]
    #[tagged_serde = 2]
    DeleteDead,
    #[tagged_serde = 3]
    DeleteSpecific,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddToStoreNar {
    pub path: StorePath,
    pub deriver: OptionalStorePath,
    pub nar_hash: NixString,
    pub references: StorePathSet,
    pub registration_time: Time,
    pub nar_size: u64,
    pub ultimate: bool,
    pub sigs: StringSet,
    pub content_address: RenderedContentAddress,
    pub repair: bool,
    pub dont_check_sigs: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FindRootsResponse {
    pub roots: Vec<(Path, StorePath)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryValidPaths {
    pub paths: StorePathSet,
    pub builders_use_substitutes: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddMultipleToStore {
    pub repair: bool,
    pub dont_check_sigs: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidPathInfo {
    pub deriver: OptionalStorePath,
    pub hash: NarHash,
    pub references: StorePathSet,
    pub registration_time: Time, // In seconds, since the epoch
    pub nar_size: u64,
    pub ultimate: bool,
    pub sigs: StringSet,
    pub content_address: RenderedContentAddress, // Can be empty
}

type RenderedContentAddress = NixString;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerifyStore {
    pub check_contents: bool,
    pub repair: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddSignatures {
    pub path: StorePath,
    pub signatures: StringSet,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddBuildLog {
    pub path: StorePath,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BuildDerivation {
    store_path: StorePath,
    derivation: Derivation,
    build_mode: BuildMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Derivation {
    pub outputs: Vec<(NixString, DerivationOutput)>,
    pub input_sources: StorePathSet,
    pub platform: NixString,
    pub builder: Path,
    pub args: StringSet,
    pub env: Vec<(NixString, NixString)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DerivationOutput {
    store_path: StorePath,
    method_or_hash: NixString,
    hash_or_impure: NixString,
}

#[cfg(test)]
mod tests {
    use serde_bytes::ByteBuf;

    use crate::{serialize::NixSerializer, worker_op::SetOptions};

    use super::*;

    #[test]
    fn test_serialize() {
        let options = SetOptions {
            keep_failing: true,
            keep_going: false,
            try_fallback: true,
            verbosity: Verbosity::Vomit,
            max_build_jobs: 77,
            max_silent_time: 77,
            _use_build_hook: 77,
            build_verbosity: Verbosity::Error,
            _log_type: 77,
            _print_build_trace: 77,
            build_cores: 77,
            use_substitutes: false,
            options: vec![(
                NixString(ByteBuf::from(b"buf1".to_owned())),
                NixString(ByteBuf::from(b"buf2".to_owned())),
            )],
        };
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut serializer = NixSerializer { write: &mut cursor };
        options.serialize(&mut serializer).unwrap();

        cursor.set_position(0);
        let mut deserializer = NixDeserializer { read: &mut cursor };
        assert_eq!(options, SetOptions::deserialize(&mut deserializer).unwrap());
    }
}
