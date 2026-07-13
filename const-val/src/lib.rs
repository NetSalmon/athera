use proc_macro::TokenStream;
use quote::ToTokens;
use syn::{parse_macro_input, Type, TypePath, TypeReference};

#[proc_macro_attribute]
pub fn const_val(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item = parse_macro_input!(item as syn::ItemConst);

    let vis = &item.vis;
    let name = &item.ident;
    let ty = &item.ty;
    let value = &item.expr;

    match ty.as_ref() {
        Type::Path(TypePath { path, .. }) => {
            let ident = &path.segments.last().unwrap().ident;

            match ident.to_string().as_str() {
                "usize" | "u8" | "u16" | "u32" | "u64" | "u128" |
                "isize" | "i8" | "i16" | "i32" | "i64" | "i128" => {
                    let mut function = String::from("parse_digit_");
                    function.push_str(ty.to_token_stream().to_string().as_str());
                    let function = syn::Ident::new(function.as_str(), name.span());

                    let name_str = name.to_string();

                    let ret = quote::quote! {
                        #vis const #name: #ty = {
                            match option_env!( #name_str ) {
                                Some(v) => const_num::#function(v, #value),
                                None => #value,
                            }
                        };
                    };

                    ret.into()
                }

                _ => panic!("not supported"),
            }
        }
        Type::Reference(TypeReference { elem, .. }) => {
            match elem.as_ref() {
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
            }

        }
        _ => panic!("not supported"),
    }
}
