use anyhow::anyhow;
use num_derive::FromPrimitive;
use num_traits::FromPrimitive;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::io::{Read, Write};

use crate::{
    serialize::{NixDeserializer, NixSerializer},
    FramedData, NarHash, NixString, Path, Result, StorePathSet, StringSet, ValidPathInfoWithPath,
};

#[derive(Debug, FromPrimitive)]
pub enum WorkerOpCode {
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

/// A zero-sized marker type. Its job is to mark the expected response
/// type for each worker op.
#[derive(Debug)]
pub struct Resp<T> {
    marker: std::marker::PhantomData<T>,
}

impl<T> Resp<T> {
    fn new() -> Resp<T> {
        Resp {
            marker: std::marker::PhantomData,
        }
    }

    fn ty(&self, v: T) -> T {
        v
    }
}

/// The different worker ops.
///
/// On the wire, they are represented as the opcode followed by the body.
///
/// TODO: It would be neat if we could just derive the serialize/deserialize
/// implementations, since this is a common pattern.
/// We'd like to write this definition like:
///
/// ```ignore
/// pub enum WorkerOp {
///    #[nix_enum(tag = 1)]
///    IsValidPath(Path, Resp<bool>),
///    #[nix_enum(tag = 2)]
///    HasSubstitutes(Todo, Resp<Todo>),
/// // ...
/// }
/// ```
///
/// and then just get rid of the Opcode enum above.
///
/// The second argument in each variant is a tag denoting the expected return value.
#[derive(Debug)]
pub enum WorkerOp {
    IsValidPath(Path, Resp<bool>),
    HasSubstitutes(Todo, Resp<Todo>),
    QueryReferrers(Todo, Resp<Todo>),
    AddToStore(AddToStore, Resp<ValidPathInfoWithPath>),
    BuildPaths(Todo, Resp<Todo>),
    EnsurePath(Path, Resp<u64>),
    AddTempRoot(Path, Resp<u64>),
    AddIndirectRoot(Todo, Resp<Todo>),
    SyncWithGC(Todo, Resp<Todo>),
    FindRoots(Todo, Resp<Todo>),
    SetOptions(SetOptions, Resp<()>),
    CollectGarbage(Todo, Resp<Todo>),
    QuerySubstitutablePathInfo(Todo, Resp<Todo>),
    QueryAllValidPaths(Todo, Resp<Todo>),
    QueryFailedPaths(Todo, Resp<Todo>),
    ClearFailedPaths(Todo, Resp<Todo>),
    QueryPathInfo(Path, Resp<QueryPathInfoResponse>),
    QueryPathFromHashPart(Todo, Resp<Todo>),
    QuerySubstitutablePathInfos(Todo, Resp<Todo>),
    QueryValidPaths(Todo, Resp<Todo>),
    QuerySubstitutablePaths(Todo, Resp<Todo>),
    QueryValidDerivers(Todo, Resp<Todo>),
    OptimiseStore(Todo, Resp<Todo>),
    VerifyStore(Todo, Resp<Todo>),
    BuildDerivation(Todo, Resp<Todo>),
    AddSignatures(Todo, Resp<Todo>),
    NarFromPath(Todo, Resp<Todo>),
    AddToStoreNar(Todo, Resp<Todo>),
    QueryMissing(QueryMissing, Resp<QueryMissingResponse>),
    QueryDerivationOutputMap(Todo, Resp<Todo>),
    RegisterDrvOutput(Todo, Resp<Todo>),
    QueryRealisation(Todo, Resp<Todo>),
    AddMultipleToStore(Todo, Resp<Todo>),
    AddBuildLog(Todo, Resp<Todo>),
    BuildPathsWithResults(BuildPathsWithResults, Resp<Vec<BuildResult>>),
}

macro_rules! for_each_op {
    ($macro_name:ident !) => {
        $macro_name!(
            IsValidPath,
            HasSubstitutes,
            QueryReferrers,
            AddToStore,
            BuildPaths,
            EnsurePath,
            AddTempRoot,
            AddIndirectRoot,
            SyncWithGC,
            FindRoots,
            SetOptions,
            CollectGarbage,
            QuerySubstitutablePathInfo,
            QueryAllValidPaths,
            QueryFailedPaths,
            ClearFailedPaths,
            QueryPathInfo,
            QueryPathFromHashPart,
            QuerySubstitutablePathInfos,
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

impl WorkerOp {
    /// Reads a worker op from the wire protocol.
    pub fn read(mut r: impl Read) -> Result<Self> {
        let mut de = NixDeserializer { read: &mut r };
        let opcode = u64::deserialize(&mut de)?;
        let opcode = WorkerOpCode::from_u64(opcode)
            .ok_or_else(|| anyhow!("invalid worker op code {opcode}"))?;

        macro_rules! op {
            ($($name:ident),*) => {
                match opcode {
                    $(WorkerOpCode::$name => Ok(WorkerOp::$name(<_>::deserialize(&mut de)?, Resp::new()))),*,
                    op => { Err(anyhow!("unknown op code {op:?}")) }
                }
            };
        }
        let op = for_each_op!(op!)?;

        // After reading AddToStore, Nix reads from a FramedSource. Since we're
        // temporarily putting the FramedSource in the AddToStore, read it here.
        //
        // This will also need to be handled in AddMultipleToStore, AddToStoreNar,
        // and AddBuildLog.
        if let WorkerOp::AddToStore(mut add, _) = op {
            add.framed = FramedData::read(&mut r)?;
            Ok(WorkerOp::AddToStore(add, Resp::new()))
        } else {
            Ok(op)
        }
    }

    pub fn write(&self, mut write: impl Write) -> Result<()> {
        let mut ser = NixSerializer { write: &mut write };
        macro_rules! op {
            ($($name:ident),*) => {
                match self {
                    $(WorkerOp::$name(inner, _resp) => {
                        (WorkerOpCode::$name as u64).serialize(&mut ser)?;
                        inner.serialize(&mut ser)?;
                    },)*
                }
            };
        }

        for_each_op!(op!);

        // See the comment in WorkerOp::read
        if let WorkerOp::AddToStore(add, _resp) = self {
            add.framed.write(write)?;
        }
        Ok(())
    }

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
                match self {
                    $(WorkerOp::$name(_inner, resp) => {
                        let reply = resp.ty(<_>::deserialize(&mut deser)?);
                        eprintln!("read reply {reply:?}");

                        reply.serialize(&mut dbg_ser)?;
                        if dbg_buf != logging_read.buf {
                            eprintln!("mismatch!");
                            eprintln!("{dbg_buf:?}");
                            eprintln!("{:?}", logging_read.buf);
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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
    pub options: Vec<(NixString, NixString)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AddToStore {
    name: Path,
    cam_str: Path,
    refs: StorePathSet,
    repair: bool,
    // TODO: This doesn't really belong here. It shouldn't be read as part of a
    // worker op: it should really be streamed.
    #[serde(skip)]
    framed: FramedData,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildPathsWithResults {
    paths: Vec<Path>,
    // TODO: make this an enum
    build_mode: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryMissing {
    paths: Vec<Path>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryPathInfoResponse {
    path: Option<ValidPathInfo>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryMissingResponse {
    will_build: StorePathSet,
    will_substitute: StorePathSet,
    unknown: StorePathSet,
    download_size: u64,
    nar_size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuildResult {
    path: NixString,
    status: u64,
    error_msg: NixString,
    time_built: u64,
    is_non_deterministic: u64,
    start_time: u64,
    stop_time: u64,
    built_outputs: DrvOutputs,
}

// TODO: first NixString is a DrvOutput; second is a Realisation
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DrvOutputs(Vec<(NixString, NixString)>);

/// A struct that panics when attempting to deserialize it. For marking
/// parts of the protocol that we haven't implemented yet.
#[derive(Debug, Clone, Serialize)]
pub struct Todo {}

impl<'de> Deserialize<'de> for Todo {
    fn deserialize<D>(_deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        todo!()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidPathInfo {
    deriver: Path, // Can be empty
    hash: NarHash,
    references: StorePathSet,
    registration_time: u64, // In seconds, since the epoch
    nar_size: u64,
    ultimate: bool,
    sigs: StringSet,
    content_address: ByteBuf, // Can be empty
}

#[cfg(test)]
mod tests {
    use crate::{serialize::NixSerializer, worker_op::SetOptions};

    use super::*;

    #[test]
    fn test_serialize() {
        let options = SetOptions {
            keep_failing: 77,
            keep_going: 77,
            try_fallback: 77,
            verbosity: 77,
            max_build_jobs: 77,
            max_silent_time: 77,
            _use_build_hook: 77,
            build_verbosity: 77,
            _log_type: 77,
            _print_build_trace: 77,
            build_cores: 77,
            use_substitutes: 77,
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