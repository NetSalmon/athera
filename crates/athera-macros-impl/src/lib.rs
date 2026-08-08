//! `athera-macros` 的过程宏实现。
//!
//! - [`const_val`](macro@const_val)：编译期常量属性宏（支持
//!   min / max / multiple_of 约束，类型支持整数 / `&str` / `bool`）；
//! - [`lazy`](macro@lazy)：生成 `LazyLock` 静态（可选 `spin` 内层锁）；
//! - [`spin`](macro@spin)：生成 `SpinLock` 包装的静态；
//! - [`Id`](derive@Id)：为单字段结构体自动实现 `athera_id_alloc::Id`。
use proc_macro::TokenStream;
use quote::ToTokens;
use syn::{
    DeriveInput, Expr, Meta, Token, Type, TypePath, TypeReference,
    parse::{Parse, ParseStream, Parser},
    parse_macro_input,
    punctuated::Punctuated,
};

struct ConstValAttrs {
    max: Option<Expr>,
    min: Option<Expr>,
    multiple_of: Option<Expr>,
}

impl Parse for ConstValAttrs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(ConstValAttrs {
                max: None,
                min: None,
                multiple_of: None,
            });
        }

        let mut max = None;
        let mut min = None;
        let mut multiple_of = None;

        let metas = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        for meta in metas {
            if meta.path().is_ident("max") {
                if let Meta::NameValue(nv) = meta {
                    max = Some(nv.value);
                }
            } else if meta.path().is_ident("min") {
                if let Meta::NameValue(nv) = meta {
                    min = Some(nv.value);
                }
            } else if meta.path().is_ident("multiple_of")
                && let Meta::NameValue(nv) = meta
            {
                multiple_of = Some(nv.value);
            }
        }

        Ok(ConstValAttrs {
            max,
            min,
            multiple_of,
        })
    }
}

/// 编译期常量属性宏。
///
/// 用法：`#[const_val]` / `#[const_val(min = N, max = M, multiple_of = K)]`。
/// 把 `const` 值（支持 `usize`/`u8`/... 整数、`&str`、`bool` 类型）改写为
/// 经字符串解析的编译期常量：整数可带约束（`min` / `max` / `multiple_of`），
/// 环境变量（与常量同名）可覆盖取值，解析失败时回退到字面量。
#[proc_macro_attribute]
pub fn const_val(attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemConst);
    let attrs = parse_macro_input!(attr as ConstValAttrs);

    let vis = &item.vis;
    let name = &item.ident;
    let ty = &item.ty;
    let value = &item.expr;

    let max_val = attrs.max;
    let min_val = attrs.min;
    let multiple_of_val = attrs.multiple_of;

    match ty.as_ref() {
        Type::Path(TypePath { path, .. }) => {
            let ident = &path.segments.last().unwrap().ident;

            match ident.to_string().as_str() {
                "usize" | "u8" | "u16" | "u32" | "u64" | "u128" | "isize" | "i8" | "i16"
                | "i32" | "i64" | "i128" => {
                    let mut function = String::from("parse_digit_");
                    function.push_str(ty.to_token_stream().to_string().as_str());
                    let function = syn::Ident::new(function.as_str(), name.span());

                    let name_str = name.to_string();

                    let mut checks = quote::quote! {};

                    if let Some(ref max) = max_val {
                        checks.extend(quote::quote! {
                            const _: () = assert!(#name <= #max, concat!(#name_str, " exceeds max"));
                        });
                    }
                    if let Some(ref min) = min_val {
                        checks.extend(quote::quote! {
                            const _: () = assert!(#name >= #min, concat!(#name_str, " is below min"));
                        });
                    }
                    if let Some(ref multiple_of) = multiple_of_val {
                        checks.extend(quote::quote! {
                            const _: () = assert!(#name % #multiple_of == 0, concat!(#name_str, " not multiple of ", stringify!(#multiple_of)));
                        });
                    }

                    let ret = quote::quote! {
                        #vis const #name: #ty = {
                            #checks
                            match option_env!( #name_str ) {
                                Some(v) => athera_macros::num::#function(v, #value),
                                None => #value,
                            }
                        };
                    };

                    ret.into()
                }
                "bool" => {
                    let name_str = name.to_string();

                    let ret = quote::quote! {
                        #vis const #name: #ty = {
                            match option_env!( #name_str ) {
                                Some(v) => athera_macros::num::parse_bool(v, #value),
                                None => #value,
                            }
                        };
                    };

                    ret.into()
                }

                _ => panic!("not supported"),
            }
        }
        Type::Reference(TypeReference { elem, .. }) => match elem.as_ref() {
            Type::Path(TypePath { path, .. }) => {
                let ident = &path.segments.last().unwrap().ident;
                match ident.to_string().as_str() {
                    "str" => {
                        let name_str = name.to_string();

                        let ret = quote::quote! {
                            #vis const #name: #ty = {
                                match option_env!( #name_str ) {
                                    Some(v) => v,
                                    None => #value,
                                }
                            };
                        };

                        ret.into()
                    }
                    _ => panic!("not supported"),
                }
            }
            _ => panic!("not supported"),
        },
        _ => panic!("not supported"),
    }
}

/// 懒加载静态属性宏。
///
/// 用法：`#[lazy]` 生成 `LazyLock<T>`，`#[lazy(spin)]` 生成
/// `LazyLock<SpinLock<T>>`。展开结果引用 `crate::sync::lazy` /
/// `crate::sync::spin`，因此只适用于本内核 crate。
#[proc_macro_attribute]
pub fn lazy(attr: TokenStream, item: TokenStream) -> TokenStream {
    let tokens = parse_macro_input!(item as syn::ItemStatic);

    let vis = &tokens.vis;
    let k = tokens.ident;
    let t = tokens.ty;
    let v = tokens.expr;

    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let args = match parser.parse(attr) {
        Ok(meta_list) => meta_list,
        Err(err) => return err.to_compile_error().into(),
    };

    for meta in args {
        match meta {
            Meta::Path(path) => {
                if path.is_ident("spin") {
                    let out = quote::quote! {
                        #vis static #k: crate::sync::lazy::LazyLock< crate::sync::spin::SpinLock<#t> >
                        = crate::sync::lazy::LazyLock::new(|| crate::sync::spin::SpinLock::new(#v));
                    };

                    return out.into();
                }
            }
            _ => continue,
        }
    }

    let out = quote::quote! {
        #vis static #k: crate::sync::lazy::LazyLock< #t > = crate::sync::lazy::LazyLock::new(|| #v);
    };

    out.into()
}

/// 自旋锁静态属性宏。
///
/// 用法：`#[spin] static X: T = value;`，展开为
/// `static X: crate::sync::spin::SpinLock<T>`。
#[proc_macro_attribute]
pub fn spin(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let tokens = parse_macro_input!(item as syn::ItemStatic);

    let vis = &tokens.vis;
    let k = tokens.ident;
    let t = tokens.ty;
    let v = tokens.expr;

    let out = quote::quote! {
        #vis static #k: crate::sync::spin::SpinLock< #t > = crate::sync::spin::SpinLock::new(#v);
    };

    out.into()
}

/// 为单字段结构体自动实现 [`athera_id_alloc::Id`]，并顺带自动实现
/// `Debug`、`Clone`、`Copy`、`PartialEq`、`Eq`、`PartialOrd`、`Ord`。
///
/// 生成的 `Id` 方法（`MIN` / `MAX` / `next` / `prev` / `distance_to`）与
/// 其余 trait 全部委托给唯一字段，因此只需 `#[derive(Id)]` 一行即可，
/// 无需再手动派生其余 trait。要求：
/// - 结构体恰好一个字段，内部类型自身实现了 `athera_id_alloc::Id`
///   （如 `usize`、`u32`）以及上述被自动实现 trait 对应的能力；
/// - 不支持泛型与其余数据类型（`Id` 派生仅面向包装类型）。
///
/// ```ignore
/// use athera_id_alloc::{Id, IdAllocator};
///
/// #[derive(Id)]
/// pub struct Wrap(pub u32);
///
/// // 自动获得：Debug、Clone、Copy、PartialEq、Eq、PartialOrd、Ord、Id
/// let mut a = IdAllocator::<Wrap>::from_range(Wrap(0)..Wrap(3));
/// assert_eq!(a.alloc(), Some(Wrap(0)));
/// assert_eq!(format!("{:?}", Wrap(1)), "Wrap(1)");
/// ```
#[proc_macro_derive(Id)]
pub fn derive_id(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;

    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(input, "`Id` 派生宏不支持泛型")
            .to_compile_error()
            .into();
    }

    let (field, field_ty) = match &input.data {
        syn::Data::Struct(data) => match &data.fields {
            syn::Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                (syn::Member::Unnamed(0.into()), &fields.unnamed[0].ty)
            }
            syn::Fields::Named(fields) if fields.named.len() == 1 => {
                let ident = fields.named[0].ident.clone().expect("命名字段");
                (syn::Member::Named(ident), &fields.named[0].ty)
            }
            _ => {
                return syn::Error::new_spanned(input, "`Id` 派生宏要求恰好一个字段")
                    .to_compile_error()
                    .into();
            }
        },
        _ => {
            return syn::Error::new_spanned(input, "`Id` 派生宏仅支持结构体")
                .to_compile_error()
                .into();
        }
    };

    let (min_expr, max_expr, next_expr, prev_expr, clone_expr, debug_body) = match &field {
        syn::Member::Unnamed(_) => (
            quote::quote!(#name(<#field_ty>::MIN)),
            quote::quote!(#name(<#field_ty>::MAX)),
            quote::quote!(self.#field.next().map(Self)),
            quote::quote!(self.#field.prev().map(Self)),
            quote::quote!(Self(self.#field.clone())),
            quote::quote!(f.debug_tuple(stringify!(#name)).field(&self.#field).finish()),
        ),
        syn::Member::Named(ident) => (
            quote::quote!(#name { #ident: <#field_ty>::MIN }),
            quote::quote!(#name { #ident: <#field_ty>::MAX }),
            quote::quote!(self.#field.next().map(|v| Self { #ident: v })),
            quote::quote!(self.#field.prev().map(|v| Self { #ident: v })),
            quote::quote!(Self { #ident: self.#field.clone() }),
            quote::quote!(f.debug_struct(stringify!(#name)).field(stringify!(#ident), &self.#field).finish()),
        ),
    };

    let output = quote::quote! {
        impl ::core::fmt::Debug for #name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                #debug_body
            }
        }

        impl ::core::clone::Clone for #name {
            fn clone(&self) -> Self {
                #clone_expr
            }
        }

        impl ::core::marker::Copy for #name {}

        impl ::core::cmp::PartialEq for #name {
            fn eq(&self, other: &Self) -> bool {
                self.#field == other.#field
            }
        }

        impl ::core::cmp::Eq for #name {}

        impl ::core::cmp::PartialOrd for #name {
            fn partial_cmp(&self, other: &Self) -> ::core::option::Option<::core::cmp::Ordering> {
                ::core::cmp::PartialOrd::partial_cmp(&self.#field, &other.#field)
            }
        }

        impl ::core::cmp::Ord for #name {
            fn cmp(&self, other: &Self) -> ::core::cmp::Ordering {
                ::core::cmp::Ord::cmp(&self.#field, &other.#field)
            }
        }

        impl ::athera_id_alloc::Id for #name {
            const MIN: Self = #min_expr;
            const MAX: Self = #max_expr;

            fn next(&self) -> Option<Self> {
                #next_expr
            }

            fn prev(&self) -> Option<Self> {
                #prev_expr
            }

            fn distance_to(&self, other: &Self) -> usize {
                self.#field.distance_to(&other.#field)
            }
        }
    };
    output.into()
}
