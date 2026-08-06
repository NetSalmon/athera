//! `athera-const` 的过程宏实现。
//!
//! - [`const_val`](macro@const_val)：编译期常量属性宏（支持
//!   min / max / multiple_of 约束）；
//! - [`lazy`](macro@lazy)：生成 `LazyLock` 静态（可选 `spin` 内层锁）；
//! - [`spin`](macro@spin)：生成 `SpinLock` 包装的静态。
use proc_macro::TokenStream;
use quote::ToTokens;
use syn::{
    Expr, Meta, Token, Type, TypePath, TypeReference,
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
/// 把 `const` 值（支持 `usize`/`u8`/... 整数类型）改写为经字符串解析的
/// 编译期常量，并在满足约束（`min` / `max` / `multiple_of`）时编译通过。
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
                                Some(v) => athera_const::num::#function(v, #value),
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
