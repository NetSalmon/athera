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

/// Read a linker-defined symbol address into a local variable via inline assembly.
///
/// This macro generates a `let` binding whose value is the address of a symbol defined
/// in the linker script (e.g. `user_stack_top`, `kernel_do_no_thing`). It uses the
/// RISC-V `la` (load address) pseudo-instruction to obtain the address at runtime.
///
/// The type of the variable defaults to the architecture's pointer type but can be
/// explicitly specified (e.g. `u64`).
///
/// # Syntax
/// ```ignore
/// get_tag_address!(varname: Type = "symbol_name");
/// get_tag_address!(varname = "symbol_name");
/// ```
///
/// # Safety
/// This macro emits inline assembly (`core::arch::asm!`) which is inherently unsafe.
/// The caller must ensure the symbol name exists in the final linked binary.
///
/// # Example
/// ```ignore
/// // Read the address of the user stack top
/// get_tag_address!(stack: u64 = "user_stack_top");
/// arch::registers::gpr::Sp::write(stack);
///
/// // Read with inferred type
/// get_tag_address!(addr = "kernel_do_no_thing");
/// arch::registers::csr::Sepc::write(addr);
/// ```
#[macro_export]
macro_rules! get_tag_address {
    ($var:ident $(: $t:ty)? = $tag:literal) => {
        let $var $(: $t)?;
        unsafe { core::arch::asm!( concat!("la {}, ", $tag), out(reg) $var ) }
    };
}

/// A macro for defining numeric enums with bidirectional conversions to/from their underlying integer type.
///
/// This macro simplifies the creation of C-style integer-backed enums, automatically implementing
/// the necessary `From` and `TryFrom` traits. It supports two modes:
///
/// 1. **Strict mode** (default, without `@fallback`): The enum variants are the only valid values for the
///    integer type. Trying to convert an integer that doesn't match any variant will result in a
///    [`TryFrom`] error returning the original integer.
///    Implements [`TryFrom<$t>`] for the enum, and [`From<$enum>`] for the integer.
///
/// 2. **Fallback mode** (invoked with `@fallback`): An additional `Reserved($t)` variant is added to
///    capture any integer value that doesn't correspond to a named variant. Conversions are infallible
///    in both directions via [`From`]. This is useful when the integer may be extended in the future
///    and unknown values must be preserved.
///
/// # Syntax
///
/// Strict mode:
/// ```ignore
/// numeric! {
///     $( #[$attr] )*
///     pub enum EnumName : IntegerType {
///         $(
///             VariantName = integer_expression,
///         )*
///     }
/// }
/// ```
///
/// Fallback mode:
/// ```ignore
/// numeric! {
///     @fallback
///     $( #[$attr] )*
///     pub enum EnumName : IntegerType {
///         $(
///             VariantName = integer_expression,
///         )*
///     }
/// }
/// ```
///
/// # Example (Strict)
///
/// ```
/// use my_crate::numeric; // adjust
///
/// numeric! {
///     /// A simple status code.
///     pub enum Status : u8 {
///         Ok = 0,
///         Error = 1,
///         Pending = 2,
///     }
/// }
///
/// let s: Status = Status::Ok;
/// let val: u8 = s.into();
/// assert_eq!(val, 0);
///
/// let maybe_status = Status::try_from(1u8);
/// assert_eq!(maybe_status, Ok(Status::Error));
/// let bad = Status::try_from(3u8);
/// assert_eq!(bad, Err(3));
/// ```
///
/// # Example (Fallback)
///
/// ```
/// numeric! {
///     @fallback
///     /// An extensible color value.
///     pub enum Color : u16 {
///         Red = 0xFF00,
///         Green = 0x00FF,
///         Blue = 0x000F,
///     }
/// }
///
/// let c: Color = Color::Blue;
/// let val: u16 = c.into();
/// assert_eq!(val, 0x000F);
///
/// let unknown = Color::from(0x1234);
/// assert!(matches!(unknown, Color::Reserved(0x1234)));
/// ```
///
/// # Note
///
/// - In fallback mode, the `Reserved` variant holds the raw integer value, so pattern matching must
///   account for it.
/// - In strict mode, all integer expressions must evaluate to compile‑time constants that fit into
///   `$t`. The macro does not enforce uniqueness; duplicate values are allowed but will cause
///   ambiguous `TryFrom` conversions (the first match wins).
/// - Attributes can be applied to the enum definition (e.g., `#[derive(Debug, Clone)]`).
/// ```
#[macro_export]
macro_rules! numeric {
    ($v:vis enum $name:ident : $t:ty { $( $item:ident = $value:expr ),* $(,)? }) => {
        paste::paste! {
            #[repr($t)]
            #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
            $v enum $name {
                $( $item ),*
            }

            impl TryFrom<$t> for $name {
                type Error = $t;

                fn try_from(value: $t) -> Result<$name, $t> {
                    $( const [<$item:upper>] : $t = $value; )*
                    match value {
                        $( [<$item:upper>] => Ok($name::$item), )*
                        _ => Err(value)
                    }
                }
            }

            impl From<$name> for $t {
                fn from(value: $name) -> $t {
                    match value {
                        $( $name::$item => $value, )*
                    }
                }
            }
        }
    };
    (@fallback $v:vis enum $name:ident : $t:ty { $( $item:ident = $value:expr ),* $(,)? }) => {
        paste::paste! {
            #[repr($t)]
            #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
            $v enum $name {
                Reserved($t),
                $( $item ),*
            }

            impl From<$t> for $name {
                fn from(value: $t) -> $name {
                    $( const [<$item:upper>] : $t = $value; )*
                    match value {
                        $( [<$item:upper>] => $name::$item, )*
                        _ => $name::Reserved(value)
                    }
                }
            }

            impl From<$name> for $t {
                fn from(value: $name) -> $t {
                    match value {
                        $name::Reserved(value) => value,
                        $( $name::$item => $value, )*
                    }
                }
            }
        }
    };
}

/// Reads multiple values from a base pointer using offset-based raw pointer arithmetic.
///
/// Each variable is assigned the result of reading from `base.add(offset)` via `unsafe`.
/// The type of each variable can be optionally specified; if omitted it will be inferred
/// from context (`let x = ...`).
///
/// # Syntax
/// ```ignore
/// mem_read!(base_ptr, var1: Type => offset1, var2 => offset2, ...);
/// ```
///
/// # Safety
/// This macro uses raw pointer reads without bounds checking. The caller must ensure that
/// all offsets point to valid, properly-aligned memory within the same allocation.
///
/// # Example
/// ```ignore
/// let buf: [u8; 16] = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
/// let base = buf.as_ptr();
/// mem_read!(
///     base,
///     a: u32 => 0,   // read u32 from offset 0
///     b: u16 => 4,   // read u16 from offset 4
///     c: u8 => 8,    // read u8 from offset 8
///     d => 12,       // type inferred
/// );
/// ```
#[macro_export]
macro_rules! mem_read {
    ($base:expr, $($var:ident $(: $t:ty)? => $offset:expr),+$(,)?) => {
        $( let $var $(: $t)? = unsafe { $base.add($offset).read() }; )+
    };
}

/// Read a contiguous array of values from a base pointer into a local stack array.
///
/// This macro declares a fixed-size array (`[T; N]`) and fills it by reading `N`
/// consecutive values of type `T` starting from a given base pointer. An optional
/// element-index offset can be provided to start reading partway into the data.
///
/// # Syntax
/// ```ignore
/// // Without element offset (starts at base):
/// read_as_array!(var_name: Type => base_ptr => count);
///
/// // With element offset (starts at base_ptr + offset * size_of::<T>()):
/// read_as_array!(var_name: Type => base_ptr, element_offset => count);
/// ```
///
/// # Safety
/// Each element is read via `unsafe { ptr.add(i).read() }`. The caller must ensure
/// that the memory region `[base, base + (offset + count) * size_of::<T>())` is
/// valid and properly aligned for type `T`.
///
/// # Example
/// ```ignore
/// let sp: *const u8 = ...;   // trap frame stack pointer
/// read_as_array!(args: u64 => sp, 10 => 8);  // read 8 × u64 starting at sp + 10*8
/// println!("syscall args: {:?}", args);
/// ```
#[macro_export]
macro_rules! read_as_array {
    (@base $base:expr, $t:ty, $offset:expr) => {
        {
            let offset = $offset;
            let base = $base as *const $t;
            unsafe { base.add(offset) }
        }
    };
    (@base $base:expr, $t:ty) => {
        $base as *const $t
    };
    ($var:ident : $t:ty => $base:expr $(, $offset:expr)? => $len:expr) => {
        let mut $var : [$t; $len] = [0; $len];
        let base = read_as_array!(@base $base, $t $(, $offset)?);

        for i in 0..$len {
            $var[i] = unsafe { base.add(i).read() };
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
macro_rules! enumeration {
    ($v:vis enum $name:ident : $t:ty { $( $item:ident = $value:expr ),*$(,)? }) => {
        #[repr(transparent)]
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $v struct $name (pub $t);

        impl $name {
            $( $v const $item : Self = Self($value); )*
        }

        #[allow(unused)]
        impl Debug for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
                match *self {
                    $( Self::$item => f.write_str(concat!( stringify!($name), "::", stringify!($item))), )*
                    _ => f.write_str("unknown"),
                }
            }
        }
    }
}