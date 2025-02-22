use std::fmt;
use wasmtime_environ::{
    EntityType, Global, Memory, ModuleTypes, Table, WasmFuncType, WasmRefType, WasmValType,
};
use wasmtime_runtime::VMSharedTypeIndex;

use crate::{type_registry::RegisteredType, Engine};

pub(crate) mod matching;

// Type Representations

// Type attributes

/// Indicator of whether a global is mutable or not
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub enum Mutability {
    /// The global is constant and its value does not change
    Const,
    /// The value of the global can change over time
    Var,
}

// Value Types

/// A list of all possible value types in WebAssembly.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum ValType {
    // NB: the ordering here is intended to match the ordering in
    // `wasmtime_types::WasmType` to help improve codegen when converting.
    /// Signed 32 bit integer.
    I32,
    /// Signed 64 bit integer.
    I64,
    /// Floating point 32 bit integer.
    F32,
    /// Floating point 64 bit integer.
    F64,
    /// A 128 bit number.
    V128,
    /// A reference to a Wasm function.
    FuncRef,
    /// A reference to opaque data in the Wasm instance.
    ExternRef,
}

impl fmt::Display for ValType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ValType::I32 => write!(f, "i32"),
            ValType::I64 => write!(f, "i64"),
            ValType::F32 => write!(f, "f32"),
            ValType::F64 => write!(f, "f64"),
            ValType::V128 => write!(f, "v128"),
            ValType::ExternRef => write!(f, "externref"),
            ValType::FuncRef => write!(f, "funcref"),
        }
    }
}

impl ValType {
    /// Returns true if `ValType` matches any of the numeric types. (e.g. `I32`,
    /// `I64`, `F32`, `F64`).
    pub fn is_num(&self) -> bool {
        match self {
            ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64 => true,
            _ => false,
        }
    }

    /// Returns true if `ValType` matches either of the reference types.
    pub fn is_ref(&self) -> bool {
        match self {
            ValType::ExternRef | ValType::FuncRef => true,
            _ => false,
        }
    }

    pub(crate) fn to_wasm_type(&self) -> WasmValType {
        match self {
            Self::I32 => WasmValType::I32,
            Self::I64 => WasmValType::I64,
            Self::F32 => WasmValType::F32,
            Self::F64 => WasmValType::F64,
            Self::V128 => WasmValType::V128,
            Self::FuncRef => WasmValType::Ref(WasmRefType::FUNCREF),
            Self::ExternRef => WasmValType::Ref(WasmRefType::EXTERNREF),
        }
    }

    pub(crate) fn from_wasm_type(ty: &WasmValType) -> Self {
        match ty {
            WasmValType::I32 => Self::I32,
            WasmValType::I64 => Self::I64,
            WasmValType::F32 => Self::F32,
            WasmValType::F64 => Self::F64,
            WasmValType::V128 => Self::V128,
            WasmValType::Ref(WasmRefType::FUNCREF) => Self::FuncRef,
            WasmValType::Ref(WasmRefType::EXTERNREF) => Self::ExternRef,
            // FIXME: exposing the full function-references (and beyond)
            // proposals will require redesigning the embedder API for `ValType`
            // and types in Wasmtime. That is a large undertaking which is
            // deferred for later. The intention for now is that
            // function-references types can't show up in the "public API" of a
            // core wasm module but it can use everything internally still.
            WasmValType::Ref(_) => {
                unimplemented!("typed function references are not exposed in the public API yet")
            }
        }
    }
}

// External Types

/// A list of all possible types which can be externally referenced from a
/// WebAssembly module.
///
/// This list can be found in [`ImportType`] or [`ExportType`], so these types
/// can either be imported or exported.
#[derive(Debug, Clone)]
pub enum ExternType {
    /// This external type is the type of a WebAssembly function.
    Func(FuncType),
    /// This external type is the type of a WebAssembly global.
    Global(GlobalType),
    /// This external type is the type of a WebAssembly table.
    Table(TableType),
    /// This external type is the type of a WebAssembly memory.
    Memory(MemoryType),
}

macro_rules! accessors {
    ($(($variant:ident($ty:ty) $get:ident $unwrap:ident))*) => ($(
        /// Attempt to return the underlying type of this external type,
        /// returning `None` if it is a different type.
        pub fn $get(&self) -> Option<&$ty> {
            if let ExternType::$variant(e) = self {
                Some(e)
            } else {
                None
            }
        }

        /// Returns the underlying descriptor of this [`ExternType`], panicking
        /// if it is a different type.
        ///
        /// # Panics
        ///
        /// Panics if `self` is not of the right type.
        pub fn $unwrap(&self) -> &$ty {
            self.$get().expect(concat!("expected ", stringify!($ty)))
        }
    )*)
}

impl ExternType {
    accessors! {
        (Func(FuncType) func unwrap_func)
        (Global(GlobalType) global unwrap_global)
        (Table(TableType) table unwrap_table)
        (Memory(MemoryType) memory unwrap_memory)
    }

    pub(crate) fn from_wasmtime(
        engine: &Engine,
        types: &ModuleTypes,
        ty: &EntityType,
    ) -> ExternType {
        match ty {
            EntityType::Function(idx) => FuncType::from_wasm_func_type(engine, &types[*idx]).into(),
            EntityType::Global(ty) => GlobalType::from_wasmtime_global(ty).into(),
            EntityType::Memory(ty) => MemoryType::from_wasmtime_memory(ty).into(),
            EntityType::Table(ty) => TableType::from_wasmtime_table(ty).into(),
            EntityType::Tag(_) => unimplemented!("wasm tag support"),
        }
    }
}

impl From<FuncType> for ExternType {
    fn from(ty: FuncType) -> ExternType {
        ExternType::Func(ty)
    }
}

impl From<GlobalType> for ExternType {
    fn from(ty: GlobalType) -> ExternType {
        ExternType::Global(ty)
    }
}

impl From<MemoryType> for ExternType {
    fn from(ty: MemoryType) -> ExternType {
        ExternType::Memory(ty)
    }
}

impl From<TableType> for ExternType {
    fn from(ty: TableType) -> ExternType {
        ExternType::Table(ty)
    }
}

/// A descriptor for a function in a WebAssembly module.
///
/// WebAssembly functions can have 0 or more parameters and results.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct FuncType {
    ty: RegisteredType,
}

impl FuncType {
    /// Creates a new function descriptor from the given parameters and results.
    ///
    /// The function descriptor returned will represent a function which takes
    /// `params` as arguments and returns `results` when it is finished.
    pub fn new(
        engine: &Engine,
        params: impl IntoIterator<Item = ValType>,
        results: impl IntoIterator<Item = ValType>,
    ) -> FuncType {
        Self::from_wasm_func_type(
            engine,
            &WasmFuncType::new(
                params.into_iter().map(|t| t.to_wasm_type()).collect(),
                results.into_iter().map(|t| t.to_wasm_type()).collect(),
            ),
        )
    }

    /// Returns the list of parameter types for this function.
    #[inline]
    pub fn params(&self) -> impl ExactSizeIterator<Item = ValType> + '_ {
        self.ty.params().iter().map(ValType::from_wasm_type)
    }

    /// Returns the list of result types for this function.
    #[inline]
    pub fn results(&self) -> impl ExactSizeIterator<Item = ValType> + '_ {
        self.ty.returns().iter().map(ValType::from_wasm_type)
    }

    pub(crate) fn type_index(&self) -> VMSharedTypeIndex {
        self.ty.index()
    }

    pub(crate) fn as_wasm_func_type(&self) -> &WasmFuncType {
        &self.ty
    }

    pub(crate) fn into_registered_type(self) -> RegisteredType {
        self.ty
    }

    pub(crate) fn from_wasm_func_type(engine: &Engine, ty: &WasmFuncType) -> FuncType {
        let ty = RegisteredType::new(engine, ty);
        Self { ty }
    }

    pub(crate) fn from_shared_type_index(engine: &Engine, index: VMSharedTypeIndex) -> FuncType {
        let ty = RegisteredType::root(engine, index).expect(
            "VMSharedTypeIndex is not registered in the Engine! Wrong \
             engine? Didn't root the index somewhere?",
        );
        Self { ty }
    }
}

// Global Types

/// A WebAssembly global descriptor.
///
/// This type describes an instance of a global in a WebAssembly module. Globals
/// are local to an [`Instance`](crate::Instance) and are either immutable or
/// mutable.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct GlobalType {
    content: ValType,
    mutability: Mutability,
}

impl GlobalType {
    /// Creates a new global descriptor of the specified `content` type and
    /// whether or not it's mutable.
    pub fn new(content: ValType, mutability: Mutability) -> GlobalType {
        GlobalType {
            content,
            mutability,
        }
    }

    /// Returns the value type of this global descriptor.
    pub fn content(&self) -> &ValType {
        &self.content
    }

    /// Returns whether or not this global is mutable.
    pub fn mutability(&self) -> Mutability {
        self.mutability
    }

    pub(crate) fn to_wasm_type(&self) -> Global {
        let wasm_ty = self.content().to_wasm_type();
        let mutability = matches!(self.mutability(), Mutability::Var);
        Global {
            wasm_ty,
            mutability,
        }
    }

    /// Returns `None` if the wasmtime global has a type that we can't
    /// represent, but that should only very rarely happen and indicate a bug.
    pub(crate) fn from_wasmtime_global(global: &Global) -> GlobalType {
        let ty = ValType::from_wasm_type(&global.wasm_ty);
        let mutability = if global.mutability {
            Mutability::Var
        } else {
            Mutability::Const
        };
        GlobalType::new(ty, mutability)
    }
}

// Table Types

/// A descriptor for a table in a WebAssembly module.
///
/// Tables are contiguous chunks of a specific element, typically a `funcref` or
/// an `externref`. The most common use for tables is a function table through
/// which `call_indirect` can invoke other functions.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct TableType {
    ty: Table,
}

impl TableType {
    /// Creates a new table descriptor which will contain the specified
    /// `element` and have the `limits` applied to its length.
    ///
    /// # Panics
    ///
    /// Panics if the `element` type provided is not a reference type.
    pub fn new(element: ValType, min: u32, max: Option<u32>) -> TableType {
        TableType {
            ty: Table {
                // FIXME: the `ValType` API should be redesigned and the
                // argument to this constructor should be `RefType`.
                wasm_ty: match element {
                    ValType::FuncRef => WasmRefType::FUNCREF,
                    ValType::ExternRef => WasmRefType::EXTERNREF,
                    _ => panic!("Attempt to convert non-reference type to a reference type"),
                },
                minimum: min,
                maximum: max,
            },
        }
    }

    /// Returns the element value type of this table.
    pub fn element(&self) -> ValType {
        ValType::from_wasm_type(&WasmValType::Ref(self.ty.wasm_ty))
    }

    /// Returns minimum number of elements this table must have
    pub fn minimum(&self) -> u32 {
        self.ty.minimum
    }

    /// Returns the optionally-specified maximum number of elements this table
    /// can have.
    ///
    /// If this returns `None` then the table is not limited in size.
    pub fn maximum(&self) -> Option<u32> {
        self.ty.maximum
    }

    pub(crate) fn from_wasmtime_table(table: &Table) -> TableType {
        TableType { ty: table.clone() }
    }

    pub(crate) fn wasmtime_table(&self) -> &Table {
        &self.ty
    }
}

// Memory Types

/// A descriptor for a WebAssembly memory type.
///
/// Memories are described in units of pages (64KB) and represent contiguous
/// chunks of addressable memory.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MemoryType {
    ty: Memory,
}

impl MemoryType {
    /// Creates a new descriptor for a 32-bit WebAssembly memory given the
    /// specified limits of the memory.
    ///
    /// The `minimum` and `maximum`  values here are specified in units of
    /// WebAssembly pages, which are 64k.
    pub fn new(minimum: u32, maximum: Option<u32>) -> MemoryType {
        MemoryType {
            ty: Memory {
                memory64: false,
                shared: false,
                minimum: minimum.into(),
                maximum: maximum.map(|i| i.into()),
            },
        }
    }

    /// Creates a new descriptor for a 64-bit WebAssembly memory given the
    /// specified limits of the memory.
    ///
    /// The `minimum` and `maximum`  values here are specified in units of
    /// WebAssembly pages, which are 64k.
    ///
    /// Note that 64-bit memories are part of the memory64 proposal for
    /// WebAssembly which is not standardized yet.
    pub fn new64(minimum: u64, maximum: Option<u64>) -> MemoryType {
        MemoryType {
            ty: Memory {
                memory64: true,
                shared: false,
                minimum,
                maximum,
            },
        }
    }

    /// Creates a new descriptor for shared WebAssembly memory given the
    /// specified limits of the memory.
    ///
    /// The `minimum` and `maximum`  values here are specified in units of
    /// WebAssembly pages, which are 64k.
    ///
    /// Note that shared memories are part of the threads proposal for
    /// WebAssembly which is not standardized yet.
    pub fn shared(minimum: u32, maximum: u32) -> MemoryType {
        MemoryType {
            ty: Memory {
                memory64: false,
                shared: true,
                minimum: minimum.into(),
                maximum: Some(maximum.into()),
            },
        }
    }

    /// Returns whether this is a 64-bit memory or not.
    ///
    /// Note that 64-bit memories are part of the memory64 proposal for
    /// WebAssembly which is not standardized yet.
    pub fn is_64(&self) -> bool {
        self.ty.memory64
    }

    /// Returns whether this is a shared memory or not.
    ///
    /// Note that shared memories are part of the threads proposal for
    /// WebAssembly which is not standardized yet.
    pub fn is_shared(&self) -> bool {
        self.ty.shared
    }

    /// Returns minimum number of WebAssembly pages this memory must have.
    ///
    /// Note that the return value, while a `u64`, will always fit into a `u32`
    /// for 32-bit memories.
    pub fn minimum(&self) -> u64 {
        self.ty.minimum
    }

    /// Returns the optionally-specified maximum number of pages this memory
    /// can have.
    ///
    /// If this returns `None` then the memory is not limited in size.
    ///
    /// Note that the return value, while a `u64`, will always fit into a `u32`
    /// for 32-bit memories.
    pub fn maximum(&self) -> Option<u64> {
        self.ty.maximum
    }

    pub(crate) fn from_wasmtime_memory(memory: &Memory) -> MemoryType {
        MemoryType { ty: memory.clone() }
    }

    pub(crate) fn wasmtime_memory(&self) -> &Memory {
        &self.ty
    }
}

// Import Types

/// A descriptor for an imported value into a wasm module.
///
/// This type is primarily accessed from the
/// [`Module::imports`](crate::Module::imports) API. Each [`ImportType`]
/// describes an import into the wasm module with the module/name that it's
/// imported from as well as the type of item that's being imported.
#[derive(Clone)]
pub struct ImportType<'module> {
    /// The module of the import.
    module: &'module str,

    /// The field of the import.
    name: &'module str,

    /// The type of the import.
    ty: EntityType,
    types: &'module ModuleTypes,
    engine: &'module Engine,
}

impl<'module> ImportType<'module> {
    /// Creates a new import descriptor which comes from `module` and `name` and
    /// is of type `ty`.
    pub(crate) fn new(
        module: &'module str,
        name: &'module str,
        ty: EntityType,
        types: &'module ModuleTypes,
        engine: &'module Engine,
    ) -> ImportType<'module> {
        ImportType {
            module,
            name,
            ty,
            types,
            engine,
        }
    }

    /// Returns the module name that this import is expected to come from.
    pub fn module(&self) -> &'module str {
        self.module
    }

    /// Returns the field name of the module that this import is expected to
    /// come from.
    pub fn name(&self) -> &'module str {
        self.name
    }

    /// Returns the expected type of this import.
    pub fn ty(&self) -> ExternType {
        ExternType::from_wasmtime(self.engine, self.types, &self.ty)
    }
}

impl<'module> fmt::Debug for ImportType<'module> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportType")
            .field("module", &self.module())
            .field("name", &self.name())
            .field("ty", &self.ty())
            .finish()
    }
}

// Export Types

/// A descriptor for an exported WebAssembly value.
///
/// This type is primarily accessed from the
/// [`Module::exports`](crate::Module::exports) accessor and describes what
/// names are exported from a wasm module and the type of the item that is
/// exported.
#[derive(Clone)]
pub struct ExportType<'module> {
    /// The name of the export.
    name: &'module str,

    /// The type of the export.
    ty: EntityType,
    types: &'module ModuleTypes,
    engine: &'module Engine,
}

impl<'module> ExportType<'module> {
    /// Creates a new export which is exported with the given `name` and has the
    /// given `ty`.
    pub(crate) fn new(
        name: &'module str,
        ty: EntityType,
        types: &'module ModuleTypes,
        engine: &'module Engine,
    ) -> ExportType<'module> {
        ExportType {
            name,
            ty,
            types,
            engine,
        }
    }

    /// Returns the name by which this export is known.
    pub fn name(&self) -> &'module str {
        self.name
    }

    /// Returns the type of this export.
    pub fn ty(&self) -> ExternType {
        ExternType::from_wasmtime(self.engine, self.types, &self.ty)
    }
}

impl<'module> fmt::Debug for ExportType<'module> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExportType")
            .field("name", &self.name().to_owned())
            .field("ty", &self.ty())
            .finish()
    }
}
