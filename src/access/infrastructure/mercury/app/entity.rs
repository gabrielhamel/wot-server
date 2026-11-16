//! Typed bindings between garage messages and the installed script definitions.

use std::{any::TypeId, collections::HashMap, fmt, io, marker::PhantomData};

use glam::{Vec2, Vec3, Vec4};

use crate::access::mercury::script::{PythonValue, StringValue, Ty, TyKind, Value};
use crate::client_formats::pickle;

use super::{
    client::element::EntityMethod,
    dispatch::{EntityDispatch, MethodCall, MethodDef, ScriptDispatch},
    io_invalid_data,
    outbox::ClientOutbox,
    slots::SlotKey,
};

#[doc(hidden)]
pub use paste::paste as __paste;

/// A Rust type that values of some script types convert to and from.
pub trait ScriptType: Sized {
    /// Whether values of the given type convert to and from this one. Checked once,
    /// when a schema is bound.
    fn check(ty: &Ty) -> bool;

    /// The value of the given type, which [`Self::check`] accepted.
    fn into_value(self, ty: &Ty) -> Value;

    /// This value back, if it is one this type holds.
    fn from_value(value: Value) -> Option<Self>;
}

/// A Rust type of method arguments: a tuple of [`ScriptType`]s, implemented up to 16.
pub trait ScriptArgs: Sized {
    /// Each argument's type.
    const TYPES: &'static [TypeDecl];

    /// The arguments as values of the given types, which [`Self::TYPES`] accepted.
    fn into_values(self, tys: &[Ty]) -> Vec<Value>;

    /// The arguments back, if they are ones this type holds.
    fn from_values(values: Vec<Value>) -> Option<Self>;
}

/// A Rust type, as checked against a script type when a schema is bound.
#[derive(Clone, Copy)]
pub struct TypeDecl {
    pub check: fn(&Ty) -> bool,
    pub name: fn() -> &'static str,
}

impl TypeDecl {
    pub const fn of<T: ScriptType>() -> Self {
        Self {
            check: T::check,
            name: std::any::type_name::<T>,
        }
    }
}

/// A type's kind, through its aliases.
pub fn kind(ty: &Ty) -> &TyKind {
    match ty.kind() {
        TyKind::Alias(inner) => kind(inner),
        kind => kind,
    }
}

macro_rules! impl_scalar {
    ($($rust:ty => $variant:ident),* $(,)?) => {
        $(impl ScriptType for $rust {
            fn check(ty: &Ty) -> bool {
                matches!(kind(ty), TyKind::$variant)
            }
            fn into_value(self, _ty: &Ty) -> Value {
                Value::$variant(self)
            }
            fn from_value(value: Value) -> Option<Self> {
                match value {
                    Value::$variant(v) => Some(v),
                    _ => None,
                }
            }
        })*
    };
}

impl_scalar! {
    i8 => Int8, i16 => Int16, i32 => Int32, i64 => Int64,
    u8 => UInt8, u16 => UInt16, u32 => UInt32, u64 => UInt64,
    f32 => Float32, f64 => Float64,
    Vec2 => Vector2, Vec3 => Vector3, Vec4 => Vector4,
}

/// `BOOL`, an alias of `UINT8`.
impl ScriptType for bool {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::UInt8)
    }
    fn into_value(self, _ty: &Ty) -> Value {
        Value::UInt8(self as u8)
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::UInt8(v) => Some(v != 0),
            _ => None,
        }
    }
}

/// A `STRING` holding text.
impl ScriptType for String {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::String)
    }
    fn into_value(self, _ty: &Ty) -> Value {
        Value::String(StringValue::String(self))
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::String(StringValue::String(s)) => Some(s),
            Value::String(StringValue::Raw(raw)) => String::from_utf8(raw).ok(),
            _ => None,
        }
    }
}

/// The bytes of a `STRING` or a `PYTHON` value, whatever they hold: a compact
/// descriptor, say, or an already encoded blob.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bytes(pub Vec<u8>);

impl ScriptType for Bytes {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::String | TyKind::Python)
    }
    fn into_value(self, ty: &Ty) -> Value {
        match kind(ty) {
            TyKind::Python => Value::Python(PythonValue::Raw(self.0)),
            _ => Value::String(StringValue::Raw(self.0)),
        }
    }
    fn from_value(value: Value) -> Option<Self> {
        Some(Self(match value {
            Value::String(StringValue::Raw(raw)) | Value::Python(PythonValue::Raw(raw)) => raw,
            Value::String(StringValue::String(s)) => s.into_bytes(),
            Value::String(StringValue::Python(value))
            | Value::Python(PythonValue::Decoded(value)) => pickle::to_bytes(&value),
            _ => return None,
        }))
    }
}

/// A pickled Python value, in a `PYTHON` or a `STRING` value.
#[derive(Debug, Clone, PartialEq)]
pub struct Pickle(pub pickle::Value);

impl ScriptType for Pickle {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::String | TyKind::Python)
    }
    fn into_value(self, ty: &Ty) -> Value {
        match kind(ty) {
            TyKind::Python => Value::Python(PythonValue::Decoded(self.0)),
            _ => Value::String(StringValue::Python(self.0)),
        }
    }
    fn from_value(value: Value) -> Option<Self> {
        let unpickle = |raw: Vec<u8>| pickle::from_bytes(&raw).ok();
        Some(Self(match value {
            Value::String(StringValue::Python(value))
            | Value::Python(PythonValue::Decoded(value)) => value,
            Value::String(StringValue::Raw(raw)) | Value::Python(PythonValue::Raw(raw)) => {
                unpickle(raw)?
            }
            _ => return None,
        }))
    }
}

/// An `ARRAY` or a `TUPLE`.
impl<T: ScriptType> ScriptType for Vec<T> {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::Array(seq) | TyKind::Tuple(seq) if T::check(&seq.ty))
    }
    fn into_value(self, ty: &Ty) -> Value {
        let (TyKind::Array(seq) | TyKind::Tuple(seq)) = kind(ty) else {
            unreachable!("checked")
        };
        Value::Seq(self.into_iter().map(|v| v.into_value(&seq.ty)).collect())
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Seq(values) => values.into_iter().map(T::from_value).collect(),
            _ => None,
        }
    }
}

/// An `ARRAY` or a `TUPLE` of this size, or of any size.
impl<T: ScriptType, const N: usize> ScriptType for [T; N] {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::Array(seq) | TyKind::Tuple(seq)
            if T::check(&seq.ty) && seq.size.is_none_or(|size| size as usize == N))
    }
    fn into_value(self, ty: &Ty) -> Value {
        let (TyKind::Array(seq) | TyKind::Tuple(seq)) = kind(ty) else {
            unreachable!("checked")
        };
        Value::Seq(self.into_iter().map(|v| v.into_value(&seq.ty)).collect())
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Seq(values) => values
                .into_iter()
                .map(T::from_value)
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok(),
            _ => None,
        }
    }
}

/// A `FIXED_DICT` declaring `AllowNone`, `None` when Python's `None`.
impl<T: ScriptType> ScriptType for Option<T> {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::Dict(dict) if dict.allow_none) && T::check(ty)
    }
    fn into_value(self, ty: &Ty) -> Value {
        match self {
            Some(v) => v.into_value(ty),
            None => Value::None,
        }
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::None => Some(None),
            value => T::from_value(value).map(Some),
        }
    }
}

/// Any value, unchecked: for what has no Rust type yet.
impl ScriptType for Value {
    fn check(_ty: &Ty) -> bool {
        true
    }
    fn into_value(self, _ty: &Ty) -> Value {
        self
    }
    fn from_value(value: Value) -> Option<Self> {
        Some(value)
    }
}

/// The type of the given field of a `FIXED_DICT`, for [`script_dict!`].
#[doc(hidden)]
pub fn dict_field_ty<'t>(ty: &'t Ty, name: &str) -> Option<&'t Ty> {
    match kind(ty) {
        TyKind::Dict(dict) => dict
            .properties
            .iter()
            .find(|prop| &*prop.name == name)
            .map(|prop| &prop.ty),
        _ => None,
    }
}

/// Whether the given type is a `FIXED_DICT` of exactly these fields, each passing its
/// check, for [`script_dict!`].
#[doc(hidden)]
type FieldCheck<'a> = (&'a str, fn(&Ty) -> bool);

pub fn dict_check(ty: &Ty, fields: &[FieldCheck<'_>]) -> bool {
    match kind(ty) {
        TyKind::Dict(dict) => {
            dict.properties.len() == fields.len()
                && fields
                    .iter()
                    .all(|&(name, check)| dict_field_ty(ty, name).is_some_and(check))
        }
        _ => false,
    }
}

macro_rules! impl_args {
    ($($arg:ident $name:ident),*) => {
        impl<$($arg: ScriptType),*> ScriptArgs for ($($arg,)*) {

            const TYPES: &'static [TypeDecl] = &[$(TypeDecl::of::<$arg>()),*];

            #[allow(unused)]
            fn into_values(self, tys: &[Ty]) -> Vec<Value> {
                let ($($name,)*) = self;
                let mut tys = tys.iter();
                vec![$($name.into_value(tys.next().expect("checked"))),*]
            }

            #[allow(unused)]
            fn from_values(values: Vec<Value>) -> Option<Self> {
                let mut values = values.into_iter();
                let args = ($($arg::from_value(values.next()?)?,)*);
                values.next().is_none().then_some(args)
            }

        }
    };
}

impl_args!();
impl_args!(A0 a0);
impl_args!(A0 a0, A1 a1);
impl_args!(A0 a0, A1 a1, A2 a2);
impl_args!(A0 a0, A1 a1, A2 a2, A3 a3);
impl_args!(A0 a0, A1 a1, A2 a2, A3 a3, A4 a4);
impl_args!(A0 a0, A1 a1, A2 a2, A3 a3, A4 a4, A5 a5);
impl_args!(A0 a0, A1 a1, A2 a2, A3 a3, A4 a4, A5 a5, A6 a6);
impl_args!(A0 a0, A1 a1, A2 a2, A3 a3, A4 a4, A5 a5, A6 a6, A7 a7);

/// A method a schema declares, with its arguments' types.
pub struct MethodDecl {
    pub name: &'static str,
    pub args: &'static [TypeDecl],
}

pub trait Schema: 'static {
    /// The type's name in the script.
    const NAME: &'static str;
    /// The client methods it calls.
    const CLIENT_METHODS: &'static [MethodDecl];
    /// The base methods it receives.
    const BASE_METHODS: &'static [MethodDecl];
}

/// The schema of an entity type with a typed view for calling client methods.
pub trait EntitySchema: Schema {
    type View<'a>: EntityView<'a>;
}

/// The typed view of a connected entity.
pub trait EntityView<'a> {
    fn new(raw: RawEntity<'a>) -> Self;
}

/// Typed decoding for the declared base methods of a schema.
pub trait Methods: Sized {
    type Schema: Schema;
    /// Decode a call of the declared method at the given index.
    fn decode(index: usize, args: Vec<Value>) -> Option<Self>;
}

/// A schema bound to an entity type of the loaded script, see [`bind`].
#[derive(Debug)]
pub struct Bound {
    schema: TypeId,
    /// The exposed id of each declared client method.
    client_methods: Vec<u16>,

    base_methods: Vec<Option<u16>>,
}

impl Bound {
    /// Whether this is the given schema's binding.
    pub fn is<S: Schema>(&self) -> bool {
        self.schema == TypeId::of::<S>()
    }

    /// Decode a call of one of the entity's base methods, if it is one the
    /// given calls declare.
    pub fn decode<M: Methods>(&self, call: &MethodCall) -> Option<M> {
        if !self.is::<M::Schema>() {
            return None;
        }
        let MethodCall::Known {
            exposed_id, args, ..
        } = call
        else {
            return None;
        };

        let index = (*self.base_methods.get(*exposed_id as usize)?)?;
        M::decode(index.into(), args.clone())
    }
}

/// Why a schema did not bind, listing every member that did not.
#[derive(Debug)]
pub struct BindError {
    pub schema: &'static str,
    pub errors: Vec<String>,
}

impl fmt::Display for BindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "schema {} doesn't match the script: {}",
            self.schema,
            self.errors.join("; ")
        )
    }
}

impl std::error::Error for BindError {}

/// Bind a schema to its type's dispatch tables, checking each declared member.
pub fn bind<S: Schema>(dispatch: &EntityDispatch) -> Result<Bound, BindError> {
    let mut errors = Vec::new();

    let mut bind_methods = |kind: &str, decls: &[MethodDecl], table: &[MethodDef]| -> Vec<u16> {
        decls
            .iter()
            .filter_map(|decl| {
                let Some(exposed_id) = table.iter().position(|def| &*def.name == decl.name) else {
                    errors.push(format!("no {kind} method {}", decl.name));
                    return None;
                };
                let def = &table[exposed_id];
                if def.args.len() != decl.args.len() {
                    errors.push(format!(
                        "{kind} method {} takes {} arguments, not {}",
                        decl.name,
                        def.args.len(),
                        decl.args.len()
                    ));
                }
                for (i, (ty, arg)) in def.args.iter().zip(decl.args).enumerate() {
                    if !(arg.check)(ty) {
                        errors.push(format!(
                            "{kind} method {} argument {i} is a {}, not a {}",
                            decl.name,
                            describe(ty),
                            (arg.name)()
                        ));
                    }
                }
                Some(exposed_id as u16)
            })
            .collect()
    };

    let client_methods = bind_methods("client", S::CLIENT_METHODS, &dispatch.client_methods);
    let by_exposed_id = |exposed_ids: Vec<u16>, len: usize| {
        let mut indices = vec![None; len];
        for (index, exposed_id) in exposed_ids.into_iter().enumerate() {
            indices[exposed_id as usize] = Some(index as u16);
        }
        indices
    };
    let base_methods = by_exposed_id(
        bind_methods("base", S::BASE_METHODS, &dispatch.base_methods),
        dispatch.base_methods.len(),
    );

    if errors.is_empty() {
        Ok(Bound {
            schema: TypeId::of::<S>(),
            client_methods,
            base_methods,
        })
    } else {
        Err(BindError {
            schema: S::NAME,
            errors,
        })
    }
}

/// The schemas of entity types, or of component types, registered with an app, by type
/// id.
#[derive(Debug, Default)]
pub(crate) struct Registry {
    bindings: HashMap<u16, Bound>,
    schema_types: HashMap<TypeId, u16>,
}

impl Registry {
    /// Bind a schema to its type, see [`bind`].
    pub fn register<S: Schema>(&mut self, script: &ScriptDispatch) -> Result<(), BindError> {
        let Some((type_id, dispatch)) = script.entity_from_name(S::NAME) else {
            return Err(BindError {
                schema: S::NAME,
                errors: vec![format!("no type {}", S::NAME)],
            });
        };

        self.bindings.insert(type_id, bind::<S>(dispatch)?);
        self.schema_types.insert(TypeId::of::<S>(), type_id);
        Ok(())
    }

    /// The type of a registered schema.
    pub fn type_of<S: Schema>(&self) -> io::Result<u16> {
        self.schema_types
            .get(&TypeId::of::<S>())
            .copied()
            .ok_or_else(|| io_invalid_data(format_args!("schema {} is not registered", S::NAME)))
    }

    /// The binding of a type, if a schema is registered for it.
    pub fn bound(&self, type_id: u16) -> Option<&Bound> {
        self.bindings.get(&type_id)
    }
}

/// A type, briefly: its name, or its kind when it has none.
fn describe(ty: &Ty) -> String {
    match (ty.name(), ty.kind()) {
        ("", TyKind::Alias(inner)) => describe(inner),
        ("", TyKind::Dict(_)) => "FIXED_DICT".to_string(),
        ("", TyKind::Array(seq)) => format!("ARRAY of {}", describe(&seq.ty)),
        ("", TyKind::Tuple(seq)) => format!("TUPLE of {}", describe(&seq.ty)),
        ("", kind) => format!("{kind:?}"),
        (name, _) => name.to_string(),
    }
}

/// An entity of an app, of the given schema: what the app gave when it created it, or
/// decoded a call to it with. Stale once the entity is gone, even if another one takes
/// its place.
pub struct EntityKey<T> {
    pub(crate) slot: SlotKey,
    pub(crate) id: u32,
    _schema: PhantomData<fn() -> T>,
}

/// The schema of an [`EntityKey`] to an entity of any type.
pub enum AnyEntity {}

impl<T> EntityKey<T> {
    pub(crate) fn new(slot: SlotKey, id: u32) -> Self {
        Self {
            slot,
            id,
            _schema: PhantomData,
        }
    }

    /// The entity's id, as the protocol carries it.
    pub fn id(&self) -> EntityId {
        EntityId(self.id)
    }

    /// The same key, to an entity of any type.
    pub fn any(self) -> EntityKey<AnyEntity> {
        EntityKey::new(self.slot, self.id)
    }

    /// The same key, to an entity of the given type, which the caller checked it is.
    pub(crate) fn cast<U>(self) -> EntityKey<U> {
        EntityKey::new(self.slot, self.id)
    }
}

impl<T> Clone for EntityKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for EntityKey<T> {}

impl<T> PartialEq for EntityKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.slot == other.slot
    }
}

impl<T> Eq for EntityKey<T> {}

impl<T> std::hash::Hash for EntityKey<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.slot.hash(state);
    }
}

impl<T> fmt::Debug for EntityKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityKey({})", self.id)
    }
}

impl<T> fmt::Display for EntityKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.id, f)
    }
}

/// An entity's id as the protocol carries it, an `OBJECT_ID` say: in method arguments
/// and properties, where a key cannot go since one cannot be read back from a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId(pub u32);

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// An `INT32` or a `UINT32`.
impl ScriptType for EntityId {
    fn check(ty: &Ty) -> bool {
        matches!(kind(ty), TyKind::Int32 | TyKind::UInt32)
    }
    fn into_value(self, ty: &Ty) -> Value {
        match kind(ty) {
            TyKind::Int32 => Value::Int32(self.0 as i32),
            _ => Value::UInt32(self.0),
        }
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Int32(id) => Some(Self(id as u32)),
            Value::UInt32(id) => Some(Self(id)),
            _ => None,
        }
    }
}

#[macro_export]
macro_rules! script_dict {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $( $(#[$field_meta:meta])* $field_vis:vis $field:ident : $ty:ty ),* $(,)?
        }
    ) => {

        $(#[$meta])*
        #[allow(non_snake_case)]
        $vis struct $name {
            $( $(#[$field_meta])* $field_vis $field: $ty ),*
        }

        impl $crate::access::mercury::app::entity::ScriptType for $name {

            fn check(ty: &$crate::access::mercury::script::Ty) -> bool {
                $crate::access::mercury::app::entity::dict_check(ty, &[
                    $( (stringify!($field), <$ty as $crate::access::mercury::app::entity::ScriptType>::check) ),*
                ])
            }

            fn into_value(self, ty: &$crate::access::mercury::script::Ty) -> $crate::access::mercury::script::Value {
                let mut dict = ::std::collections::BTreeMap::new();
                $(
                    let field_ty = $crate::access::mercury::app::entity::dict_field_ty(ty, stringify!($field)).expect("checked");
                    dict.insert(::std::sync::Arc::from(stringify!($field)), $crate::access::mercury::app::entity::ScriptType::into_value(self.$field, field_ty));
                )*
                $crate::access::mercury::script::Value::Dict(dict)
            }

            fn from_value(value: $crate::access::mercury::script::Value) -> ::std::option::Option<Self> {
                let $crate::access::mercury::script::Value::Dict(mut dict) = value else { return None };
                Some(Self {
                    $( $field: $crate::access::mercury::app::entity::ScriptType::from_value(dict.remove(stringify!($field))?)? ),*
                })
            }

        }

    };
}

pub struct RawEntity<'a> {
    pub(super) dispatch: &'a EntityDispatch,
    pub(super) binding: &'a Bound,
    pub(super) outbox: &'a mut ClientOutbox,
}

impl RawEntity<'_> {
    pub fn call_client<A: ScriptArgs>(&mut self, index: usize, args: A) -> io::Result<()> {
        let exposed_id = *self
            .binding
            .client_methods
            .get(index)
            .ok_or_else(|| io_invalid_data(format_args!("unbound client method {index}")))?;
        let method = self
            .dispatch
            .client_methods
            .get(exposed_id as usize)
            .ok_or_else(|| io_invalid_data(format_args!("unknown client method {exposed_id}")))?;
        let call = MethodCall::Known {
            exposed_id,
            name: method.name.clone(),
            args: args.into_values(&method.args),
        };
        self.outbox.select_player()?;
        self.outbox
            .write_selected(EntityMethod { call }, &self.dispatch.client_methods)
    }
}

macro_rules! entity_schema {
    (
        $vis:vis $name:ident {
            client { $(fn $client:ident($( $client_arg:ident: $client_ty:ty ),* $(,)?);)* }
            base { $(fn $base:ident($( $base_arg:ident: $base_ty:ty ),* $(,)?);)* }
        }
    ) => {
        paste::paste! {
            $vis struct $name;

            impl $crate::access::mercury::app::entity::Schema for $name {
                const NAME: &'static str = stringify!($name);
                const CLIENT_METHODS: &'static [$crate::access::mercury::app::entity::MethodDecl] = &[
                    $($crate::access::mercury::app::entity::MethodDecl {
                        name: stringify!($client),
                        args: <($($client_ty,)*) as $crate::access::mercury::app::entity::ScriptArgs>::TYPES,
                    },)*
                ];
                const BASE_METHODS: &'static [$crate::access::mercury::app::entity::MethodDecl] = &[
                    $($crate::access::mercury::app::entity::MethodDecl {
                        name: stringify!($base),
                        args: <($($base_ty,)*) as $crate::access::mercury::app::entity::ScriptArgs>::TYPES,
                    },)*
                ];
            }

            impl $crate::access::mercury::app::entity::EntitySchema for $name {
                type View<'a> = [<$name View>]<'a>;
            }

            #[allow(non_camel_case_types)]
            enum [<$name ClientMethod>] { $($client,)* }

            $vis struct [<$name View>]<'a>($crate::access::mercury::app::entity::RawEntity<'a>);

            impl<'a> $crate::access::mercury::app::entity::EntityView<'a> for [<$name View>]<'a> {
                fn new(raw: $crate::access::mercury::app::entity::RawEntity<'a>) -> Self {
                    Self(raw)
                }
            }

            #[allow(non_snake_case, dead_code)]
            impl [<$name View>]<'_> {
                $(pub fn $client(&mut self $(, $client_arg: $client_ty)*) -> std::io::Result<()> {
                    self.0.call_client([<$name ClientMethod>]::$client as usize, ($($client_arg,)*))
                })*
            }

            #[allow(non_camel_case_types)]
            enum [<$name BaseMethod>] { $($base,)* }

            #[allow(dead_code)]
            #[derive(Debug)]
            $vis enum [<$name Base>] {
                $([<$base:camel>] { $($base_arg: $base_ty,)* },)*
            }

            impl $crate::access::mercury::app::entity::Methods for [<$name Base>] {
                type Schema = $name;

                fn decode(index: usize, args: Vec<$crate::access::mercury::script::Value>) -> Option<Self> {
                    $(if index == [<$name BaseMethod>]::$base as usize {
                        let ($($base_arg,)*) =
                            <($($base_ty,)*) as $crate::access::mercury::app::entity::ScriptArgs>::from_values(args)?;
                        return Some(Self::[<$base:camel>] { $($base_arg,)* });
                    })*
                    None
                }
            }
        }
    };
}

pub(crate) use entity_schema;
