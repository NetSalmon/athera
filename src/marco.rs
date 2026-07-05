/// Define a bitfield-typed newtype with getters and setters for individual bit fields.
///
/// This macro generates a `#[repr(transparent)]` wrapper struct around an integer type
/// (e.g. `u8`, `u32`, `u64`, `usize`) and provides named accessor methods for manipulating
/// specific bit ranges. Two kinds of fields are supported:
///
/// - **Single-bit field** (`name: bit_index`): produces a getter returning `bool` and a
///   setter (`set_name`) accepting `bool`.
/// - **Multi-bit field** (`name: from => to`): produces a getter returning the inner integer
///   type and a setter (`set_name`) accepting the same type. The value is automatically masked
///   to the field width.
///
/// The struct also implements:
/// - `From<$ori_type>` / `Into<$ori_type>` bidirectional conversions
/// - `Deref<Target = $ori_type>` for transparent access to the raw value
/// - `Copy`, `Clone`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`, `Debug`, `Default`
/// - A `const fn from(value) -> Self` and `const fn new() -> Self` (zero-initialized) constructor
///
/// # Syntax
/// ```ignore
/// bits! {
///     pub type Name : BaseType {
///         single_bit_field: bit_position,
///         multi_bit_field: start_bit => end_bit,
///     }
/// }
/// ```
///
/// # Example
/// ```
/// use my_crate::bits;
///
/// bits! {
///     pub type Status : u8 {
///         ready: 0,         // single bit
///         error: 1,         // single bit
///         mode: 2 => 4,     // 3-bit field
///     }
/// }
///
/// let mut s = Status::new();
/// s.set_ready(true);
/// s.set_mode(5);
/// assert!(s.ready());
/// assert_eq!(s.mode(), 5);
/// let raw: u8 = s.into();
/// assert_eq!(raw, 0b00010101);
/// ```
///
/// # Note
/// - Bit ranges are **inclusive** on both ends (e.g. `2 => 4` covers bits 2, 3, 4).
/// - Setters automatically mask the input value to the field width; out-of-range bits are
///   silently discarded.
/// - The struct name is given without a visibility prefix on its fields; the inner value
///   is always private (`$type_name(u8)` where the field is not `pub`).
#[macro_export]
macro_rules! bits {
    (@ty $from:expr, $to:expr) => { usize };
    (@ty $from:expr) => { bool };
    (@get $v:vis $part_name:ident, $from:expr, $to:expr, $ori_type:ty) => {
        #[inline]
        pub fn $part_name(&self) -> $ori_type {
            const MASK: $ori_type = ((1 << ($to - $from + 1)) - 1) << $from;
            (self.0 & MASK) >> $from
        }
    };
    (@get $v:vis $part_name:ident, $from:expr, $ori_type:ty) => {
        #[inline]
        pub fn $part_name(&self) -> bool {
            const MASK: $ori_type = 1 << $from;
            (self.0 & MASK) != 0
        }
    };
    (@set $v:vis $part_name:ident, $from:expr, $to:expr, $ori_type:ty) => {
        paste::paste! {
            #[inline]
            pub fn [<set_ $part_name>](&mut self, value: $ori_type) {
                const CLR_MASK: $ori_type = !(((1 << ($to - $from + 1)) - 1) << $from);
                let res = (self.0 & CLR_MASK) | ((value & ((1 << ($to - $from + 1)) - 1)) << $from);
                self.0 = res as $ori_type;
            }
        }
    };
    (@set $v:vis $part_name:ident, $from:expr, $ori_type:ty) => {
        paste::paste! {
            #[inline]
            pub fn [<set_ $part_name>](&mut self, value: bool) {
                const CLR_MASK: $ori_type = !(1 << $from);
                let res = (self.0 & CLR_MASK) | ((if value {1} else {0}) << $from);
                self.0 = res as $ori_type;
            }
        }
    };
    (
        $v:vis type $type_name:ident : $ori_type:ty {
            $($part_name:ident : $from:expr $(=> $to:expr)?),* $(,)?
        }
    ) => {
        paste::paste! {
            #[repr(transparent)]
            #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
            $v struct $type_name($ori_type);

            impl $type_name {
                pub const fn from(value: $ori_type) -> Self {
                    Self(value)
                }

                pub const fn new() -> Self { Self(0) }

                $(
                    bits!(@get $v $part_name, $from $(, $to)?, $ori_type);
                    bits!(@set $v $part_name, $from $(, $to)?, $ori_type);
                )*
            }

            impl From<$ori_type> for $type_name {
                fn from(value: $ori_type) -> Self {
                    Self(value)
                }
            }

            impl From<$type_name> for $ori_type {
                fn from(value: $type_name) -> $ori_type {
                    value.0
                }
            }

            impl core::ops::Deref for $type_name {
                type Target = $ori_type;
                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }
        }
    };
}

/// Define a transparent wrapper struct over a fixed-size array with named field accessors.
///
/// This macro generates a `#[repr(transparent)]` struct wrapping a public array (`[T; N]`)
/// and provides named getter and setter methods for individual elements by index.
/// Each field's type can optionally differ from the inner array element type via an
/// automatic `.into()` / `.try_into()` conversion.
///
/// Three field kinds are supported:
///
/// | Syntax | Getter signature | Setter signature |
/// |--------|-----------------|------------------|
/// | `name => index` | `fn name(&self) -> T` | `fn set_name(&mut self, value: T)` |
/// | `name: ToType => index` | `fn name(&self) -> ToType` (via `.into()`) | `fn set_name(&mut self, value: ToType)` (via `.into()`) |
/// | `name: @try ToType => index` | `fn name(&self) -> Result<ToType, T>` (via `.try_into()`) | `fn set_name(&mut self, value: T)` (no conversion) |
///
/// # Syntax
/// ```ignore
/// array_struct! {
///     pub struct StructName : [ElementType; SIZE] {
///         field_name: @try ConvertedType => array_index,
///         field_name: ConvertedType => array_index,
///         field_name => array_index,
///     }
/// }
/// ```
///
/// # Example
/// ```
/// use my_crate::array_struct;
///
/// array_struct! {
///     pub struct EIdent : [u8; 16] {
///         class: @try Class => 4,     // TryFrom conversion, may fail
///         data: @try Endianess => 5,
///         version => 6,               // direct u8 access
///         os_abi: OsAbi => 7,          // infallible Into conversion
///         abi_version => 8,
///     }
/// }
/// ```
///
/// # Note
/// - The inner array is `pub`, so direct indexing (`self.0[i]`) is always possible.
/// - `@try` fields return `Result<ToType, T>` to handle conversions that may fail.
/// - Non-`@try` fields with a conversion type use `.into()`, which must be infallible.
#[macro_export]
macro_rules! array_struct {
    (@getter $item:ident, $index:expr, $t:ty, ) => {
        paste::paste! {
            pub fn [<$item:snake>](&self) -> $t {
                self.0[$index]
            }
        }
    };
    (@getter $item:ident, $index:expr, $t:ty, $to_ty:ty ) => {
        paste::paste! {
            pub fn [<$item:snake>](&self) -> $to_ty {
                self.0[$index].into()
            }
        }
    };
    (@getter $item:ident, $index:expr, $t:ty, @ try $to_ty:ty) => {
        paste::paste! {
            pub fn [<$item:snake>](&self) -> Result<$to_ty, $t> {
                self.0[$index].try_into()
            }
        }
    };
    (@setter $item:ident, $index:expr, $t:ty, ) => {
        paste::paste! {
            pub fn [<set_ $item:snake>](&mut self, value: $t) {
                self.0[$index] = value;
            }
        }
    };
    (@setter $item:ident, $index:expr, $t:ty, $to_ty:ty) => {
        paste::paste! {
            pub fn [<set_ $item:snake>](&mut self, value: $to_ty) {
                self.0[$index] = value.into();
            }
        }
    };
    ($v:vis struct $name:ident : [$t:ty; $l:expr] { $($item:ident $(: $(@ $try:tt)? $to_ty:ty)? => $index:expr),+$(,)? }) => {
        paste::paste! {
            #[repr(transparent)]
            #[derive(Debug)]
            $v struct $name ( pub [$t; $l] );

            #[allow(unused)]
            impl $name {
                $(
                array_struct!(@getter $item, $index, $t, $($(@ $try)? $to_ty)?);
                array_struct!(@setter $item, $index, $t, $($to_ty)?);
                )*
            }
        }
    };
}

#[macro_export]
macro_rules! numeric {
    ($v:vis enum $name:ident with ops : $t:ty { $( $item:ident = $value:expr ),*$(,)? }) => {
        numeric!($v enum $name : $t { $( $item = $value, )* });

        impl core::ops::Add for $name {
            type Output = $name;

            fn add(self, rhs: Self) -> Self::Output {
                $name(self.0 + rhs.0)
            }
        }

        impl core::ops::Sub for $name {
            type Output = $name;
            fn sub(self, rhs: Self) -> Self::Output {
                $name(self.0 - rhs.0)
            }
        }

        impl core::ops::Add<$t> for $name {
            type Output = $t;

            fn add(self, rhs: $t) -> Self::Output {
                self.0 + rhs
            }
        }

        impl core::ops::Add<$name> for $t {
            type Output = $t;

            fn add(self, rhs: $name) -> Self::Output {
                rhs.0 + self
            }
        }

        impl core::ops::Sub<$t> for $name {
            type Output = $t;

            fn sub(self, rhs: $t) -> Self::Output {
                self.0 - rhs
            }
        }

        impl core::ops::Sub<$name> for $t {
            type Output = $t;

            fn sub(self, rhs: $name) -> Self::Output {
                rhs.0 - self
            }
        }

        impl core::ops::Mul<$t> for $name {
            type Output = $t;

            fn mul(self, rhs: $t) -> Self::Output {
                self.0 * rhs
            }
        }

        impl core::ops::Mul<$name> for $t {
            type Output = $t;

            fn mul(self, rhs: $name) -> Self::Output {
                rhs.0 * self
            }
        }

        impl core::ops::Div<$t> for $name {
            type Output = $t;

            fn div(self, rhs: $t) -> Self::Output {
                self.0 / rhs
            }
        }

        impl core::ops::Div<$name> for $t {
            type Output = $t;

            fn div(self, rhs: $name) -> Self::Output {
                self / rhs.0
            }
        }
    };
    ($v:vis enum $name:ident : $t:ty { $( $item:ident = $value:expr ),*$(,)? }) => {
        #[repr(transparent)]
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $v struct $name (pub $t);

        impl $name {
            $( $v const $item : Self = Self($value); )*
        }

        #[allow(unused)]
        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                match *self {
                    $( Self::$item => f.write_str(concat!( stringify!($name), "::", stringify!($item))), )*
                    _ => f.write_str("unknown"),
                }
            }
        }

        impl From<$t> for $name {
            fn from(value: $t) -> Self {
                $name(value)
            }
        }

        impl From<$name> for $t {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    }
}
